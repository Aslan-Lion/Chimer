//! End-to-end tests for the relay's transport.
//!
//! These drive the real listener on an ephemeral loopback port, so the router,
//! both WebSocket upgrades, the two session state machines and the routing
//! table are exercised together over real sockets — the same stack the binary
//! starts, not a re-implementation of it.
//!
//! Every frame a *client* sends is written as literal JSON rather than through
//! `wire`'s encoders. That is deliberate: it pins the wire format independently,
//! so a change to the Rust types cannot make these tests agree with a bug.
//!
//! The transport here is plaintext, which the configuration only permits on a
//! loopback address. TLS termination and certificate validation are covered
//! elsewhere and are not re-tested here.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use app_timer_relay::config::Config;
use app_timer_relay::http::AppState;
use app_timer_relay::registry::Registry;
use app_timer_relay::wire::{self, codes, DeviceOutbound, HostOutbound};
use app_timer_relay::{serve, state};

use axum_server::Handle;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

const HOST_ID: &str = "h-testhost";
const HOST_CREDENTIAL: &str = "correct-horse-battery-staple";
const OTHER_HOST_ID: &str = "h-otherhost";
const OTHER_CREDENTIAL: &str = "another-long-secret-value";

/// How long a test waits for a frame that is expected to arrive.
const PATIENCE: Duration = Duration::from_secs(5);

/// How long a test waits before concluding that nothing is going to arrive.
const QUIET: Duration = Duration::from_millis(300);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A relay running on this machine, on a port the operating system chose.
struct Relay {
    address: SocketAddr,
    handle: Handle<SocketAddr>,
    state: AppState,
}

impl Relay {
    /// Starts a relay from a set of environment variables.
    async fn start(pairs: &[(&str, &str)]) -> Relay {
        let mut vars: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        // Plaintext on a loopback port is the only shape the configuration
        // allows without certificates, which keeps these tests hermetic.
        vars.insert("RELAY_ALLOW_INSECURE".to_string(), "1".to_string());
        vars.insert("RELAY_BIND".to_string(), "127.0.0.1:0".to_string());

        let config = Config::parse(vars).expect("the test configuration must be valid");
        let state = state(config);
        let handle = Handle::new();

        let serving = handle.clone();
        let listed = state.clone();
        tokio::spawn(async move {
            let _ = serve(serving, listed, None).await;
        });

        let address = handle
            .listening()
            .await
            .expect("the relay must report the address it bound to");

        Relay {
            address,
            handle,
            state,
        }
    }

    /// A relay that serves the two hosts the tests use.
    async fn with_hosts() -> Relay {
        let hosts = format!(
            "{HOST_ID}:{HOST_CREDENTIAL},{OTHER_HOST_ID}:{OTHER_CREDENTIAL}"
        );
        Relay::start(&[("RELAY_HOSTS", hosts.as_str())]).await
    }

    fn host_url(&self) -> String {
        format!("ws://{}/ws/host", self.address)
    }

    fn device_url(&self, host_id: &str) -> String {
        format!("ws://{}/ws/device/{host_id}", self.address)
    }

    fn health_url(&self) -> String {
        format!("http://{}/healthz", self.address)
    }

    /// The same registry the listener is routing through.
    fn registry(&self) -> &Registry {
        &self.state.registry
    }
}

// `Handle` is the shutdown trigger; dropping the test's relay stops it.
impl Drop for Relay {
    fn drop(&mut self) {
        self.handle.graceful_shutdown(None);
    }
}

async fn host_socket(relay: &Relay) -> Socket {
    connect_async(relay.host_url())
        .await
        .expect("the host upgrade must succeed")
        .0
}

async fn device_socket(relay: &Relay, host_id: &str) -> Socket {
    connect_async(relay.device_url(host_id))
        .await
        .expect("the device upgrade must succeed")
        .0
}

/// Registers a host and asserts the relay accepted it.
async fn register_host(relay: &Relay, host_id: &str, credential: &str) -> Socket {
    let mut socket = host_socket(relay).await;
    let hello = json!({
        "t": "hello",
        "protocol": wire::RELAY_PROTOCOL,
        "hostId": host_id,
        "hostToken": credential,
    });
    send(&mut socket, &hello).await;

    match next_host(&mut socket).await {
        HostOutbound::Ready { host_id: got, .. } => assert_eq!(got, host_id),
        other => panic!("the relay did not accept the host: {other:?}"),
    }

    socket
}

/// Attaches a device and returns its socket and the route it was given.
async fn attach_device(relay: &Relay, host_id: &str, host: &mut Socket) -> (Socket, String) {
    let mut device = device_socket(relay, host_id).await;
    send(&mut device, &json!({"t": "hello"})).await;

    // The host is told a device arrived before the device is told anything.
    let route = match next_host(host).await {
        HostOutbound::Open { route, .. } => route,
        other => panic!("the host was not told about the device: {other:?}"),
    };
    assert!(wire::valid_route_id(&route), "route {route:?} is not well formed");

    match next_device(&mut device).await {
        DeviceOutbound::Ready {
            route: got,
            host_online,
            ..
        } => {
            assert_eq!(got, route);
            assert!(host_online);
        }
        other => panic!("the device was not accepted: {other:?}"),
    }

    (device, route)
}

async fn send(socket: &mut Socket, message: &Value) {
    socket
        .send(Message::text(message.to_string()))
        .await
        .expect("the frame must be written to the socket");
}

/// Reads envelopes until one arrives, ignoring keep-alive traffic.
async fn next_host(socket: &mut Socket) -> HostOutbound {
    decode(&next_text(socket).await)
}

async fn next_device(socket: &mut Socket) -> DeviceOutbound {
    decode(&next_text(socket).await)
}

fn decode<T: serde::de::DeserializeOwned>(text: &str) -> T {
    serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("{text} is not a valid relay envelope ({error})"))
}

async fn next_text(socket: &mut Socket) -> String {
    loop {
        match tokio::time::timeout(PATIENCE, socket.next()).await {
            Err(_) => panic!("no frame arrived within {PATIENCE:?}"),
            Ok(None) => panic!("the socket closed before a frame arrived"),
            Ok(Some(Err(error))) => panic!("the socket failed: {error}"),
            // The relay's own keep-alives are not application traffic.
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            Ok(Some(Ok(Message::Text(text)))) => return text.as_str().to_string(),
            Ok(Some(Ok(other))) => panic!("unexpected frame: {other:?}"),
        }
    }
}

/// True when no text frame arrives within [`QUIET`].
async fn silent(socket: &mut Socket) -> bool {
    let deadline = tokio::time::Instant::now() + QUIET;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return true;
        }
        match tokio::time::timeout(remaining, socket.next()).await {
            Err(_) => return true,
            Ok(Some(Ok(Message::Text(_)))) => return false,
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(_))) | Ok(None) => return true,
        }
    }
}

/// Waits until `condition` holds, or gives up after [`PATIENCE`].
///
/// Some state changes happen on the relay's own task *after* a socket closes: a
/// host is unregistered by the task that owned it, not by the client's `drop`.
/// Polling the observable state waits for the real event instead of sleeping
/// for a guessed interval, so the test is neither flaky nor needlessly slow.
async fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if condition() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A minimal HTTP/1.1 `GET`, so the health check is tested over a real socket
/// rather than through an in-process function call.
async fn http_get(url: &str) -> (u16, String) {
    let rest = url.strip_prefix("http://").expect("only http is used here");
    let (authority, path) = rest.split_once('/').expect("the URL must carry a path");
    let mut stream = TcpStream::connect(authority)
        .await
        .expect("the health port must accept a connection");

    let request = format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("the request must be written");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("the response must be read");
    let response = String::from_utf8_lossy(&response).into_owned();

    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("a response must have a header block");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("the status line must carry a status code");

    (status, body.to_string())
}

// ---------------------------------------------------------------------------
// Host registration and authentication
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_host_registers_with_its_credential() {
    let relay = Relay::with_hosts().await;
    let host = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;
    drop(host);

    // The host is unregistered by the task that owned its socket, once that
    // socket closes - not by `drop`. Wait for the relay to observe the
    // disconnection instead of sleeping for a guessed interval.
    assert!(
        wait_until(|| relay.registry().stats().hosts == 0).await,
        "the host has gone"
    );
}

#[tokio::test]
async fn a_host_with_the_wrong_credential_is_refused() {
    let relay = Relay::with_hosts().await;
    let mut host = host_socket(&relay).await;
    let hello = json!({
        "t": "hello",
        "protocol": wire::RELAY_PROTOCOL,
        "hostId": HOST_ID,
        "hostToken": "not-the-shared-secret",
    });
    send(&mut host, &hello).await;

    match next_host(&mut host).await {
        HostOutbound::Error { code, .. } => assert_eq!(code, codes::UNAUTHORIZED_HOST),
        other => panic!("a bad credential must be refused, got {other:?}"),
    }
    assert_eq!(relay.registry().stats().hosts, 0);
}

#[tokio::test]
async fn an_unconfigured_host_id_is_refused_the_same_way() {
    let relay = Relay::with_hosts().await;
    let mut host = host_socket(&relay).await;
    let hello = json!({
        "t": "hello",
        "protocol": wire::RELAY_PROTOCOL,
        // Well formed, but not one of the relay's hosts.
        "hostId": "h-notconfigured",
        "hostToken": HOST_CREDENTIAL,
    });
    send(&mut host, &hello).await;

    // The same code as a bad credential: the relay never confirms which host
    // identifiers exist.
    match next_host(&mut host).await {
        HostOutbound::Error { code, .. } => assert_eq!(code, codes::UNAUTHORIZED_HOST),
        other => panic!("an unknown host must be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_second_socket_for_one_host_id_is_refused() {
    let relay = Relay::with_hosts().await;
    let _first = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;

    let mut second = host_socket(&relay).await;
    let hello = json!({
        "t": "hello",
        "protocol": wire::RELAY_PROTOCOL,
        "hostId": HOST_ID,
        "hostToken": HOST_CREDENTIAL,
    });
    send(&mut second, &hello).await;

    match next_host(&mut second).await {
        HostOutbound::Error { code, .. } => assert_eq!(code, codes::HOST_CONFLICT),
        other => panic!("a duplicate host socket must be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_garbage_first_frame_is_answered_with_malformed() {
    let relay = Relay::with_hosts().await;
    let mut host = host_socket(&relay).await;

    host.send(Message::text("this is not a relay envelope"))
        .await
        .expect("the frame must be written");

    match next_host(&mut host).await {
        HostOutbound::Error { code, .. } => assert_eq!(code, codes::MALFORMED),
        other => panic!("expected a malformed report, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Device registration and routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_device_is_routed_to_its_host_in_both_directions() {
    let relay = Relay::with_hosts().await;
    let mut host = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;
    let (mut device, route) = attach_device(&relay, HOST_ID, &mut host).await;

    // Device to host, unchanged.
    let command = json!({"type": "command", "commandId": "c-1", "action": "pause"});
    send(&mut device, &json!({"t": "data", "payload": command})).await;
    match next_host(&mut host).await {
        HostOutbound::Data { route: got, payload } => {
            assert_eq!(got, route);
            assert_eq!(payload, command, "a forwarded payload must not be altered");
        }
        other => panic!("expected data, got {other:?}"),
    }

    // Host to device, unchanged.
    let snapshot = json!({"type": "snapshot", "revision": 7});
    let frame = json!({"t": "data", "route": route, "payload": snapshot});
    send(&mut host, &frame).await;
    match next_device(&mut device).await {
        DeviceOutbound::Data { payload } => assert_eq!(payload, snapshot),
        other => panic!("expected data, got {other:?}"),
    }
}

#[tokio::test]
async fn one_host_never_sees_another_hosts_traffic() {
    let relay = Relay::with_hosts().await;
    let mut first = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;
    let mut second = register_host(&relay, OTHER_HOST_ID, OTHER_CREDENTIAL).await;

    let (mut device, _route) = attach_device(&relay, HOST_ID, &mut first).await;
    let secret = json!({"type": "command", "commandId": "c-2"});
    send(&mut device, &json!({"t": "data", "payload": secret})).await;

    match next_host(&mut first).await {
        HostOutbound::Data { .. } => {}
        other => panic!("the device's own host must receive the frame, got {other:?}"),
    }
    assert!(
        silent(&mut second).await,
        "the other host must receive nothing at all"
    );
}

#[tokio::test]
async fn a_device_for_an_unconfigured_host_is_refused_without_routing() {
    let relay = Relay::with_hosts().await;
    let mut device = device_socket(&relay, "h-notconfigured").await;
    send(&mut device, &json!({"t": "hello"})).await;

    match next_device(&mut device).await {
        DeviceOutbound::Error { code, .. } => assert_eq!(code, codes::UNKNOWN_HOST),
        other => panic!("expected an unknown-host refusal, got {other:?}"),
    }
    assert_eq!(relay.registry().stats().devices, 0, "no route was opened");
}

#[tokio::test]
async fn a_device_is_told_when_its_host_is_not_connected() {
    let relay = Relay::with_hosts().await;
    let mut device = device_socket(&relay, HOST_ID).await;
    send(&mut device, &json!({"t": "hello"})).await;

    match next_device(&mut device).await {
        DeviceOutbound::Error { code, .. } => assert_eq!(code, codes::HOST_OFFLINE),
        other => panic!("expected a host-offline refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_malformed_host_id_never_reaches_a_websocket() {
    let relay = Relay::with_hosts().await;
    let refused = connect_async(relay.device_url("not-a-host-id"))
        .await
        .expect_err("the upgrade must be refused before the handshake");

    match refused {
        WsError::Http(response) => assert_eq!(response.status().as_u16(), 404),
        other => panic!("expected an HTTP refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_device_that_goes_away_closes_its_route_on_the_host() {
    let relay = Relay::with_hosts().await;
    let mut host = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;
    let (device, route) = attach_device(&relay, HOST_ID, &mut host).await;

    drop(device);

    match next_host(&mut host).await {
        HostOutbound::Closed {
            route: got,
            reason,
        } => {
            assert_eq!(got, route);
            assert!(!reason.is_empty(), "the host is told why the route closed");
        }
        other => panic!("expected the route to close, got {other:?}"),
    }
    assert_eq!(relay.registry().stats().devices, 0);
}

#[tokio::test]
async fn several_devices_reach_one_host_on_separate_routes() {
    let relay = Relay::with_hosts().await;
    let mut host = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;

    let (mut first, first_route) = attach_device(&relay, HOST_ID, &mut host).await;
    let (mut second, second_route) = attach_device(&relay, HOST_ID, &mut host).await;
    assert_ne!(first_route, second_route, "routes must not collide");
    assert_eq!(relay.registry().stats().devices, 2);

    send(
        &mut first,
        &json!({"t": "data", "payload": {"type": "from-first"}}),
    )
    .await;
    send(
        &mut second,
        &json!({"t": "data", "payload": {"type": "from-second"}}),
    )
    .await;

    // Both arrive, each tagged with the route it came from.
    let mut seen = Vec::new();
    for _ in 0..2 {
        match next_host(&mut host).await {
            HostOutbound::Data { route, payload } => seen.push((route, payload)),
            other => panic!("expected data, got {other:?}"),
        }
    }
    seen.sort_by(|left, right| left.0.cmp(&right.0));
    let tagged: Vec<(&str, &str)> = seen
        .iter()
        .map(|(route, payload)| {
            (
                route.as_str(),
                payload["type"].as_str().expect("a type field"),
            )
        })
        .collect();
    let expected = {
        let mut pairs = vec![
            (first_route.as_str(), "from-first"),
            (second_route.as_str(), "from-second"),
        ];
        pairs.sort();
        pairs
    };
    assert_eq!(tagged, expected);
}

// ---------------------------------------------------------------------------
// Limits, shutdown and observability
// ---------------------------------------------------------------------------

#[tokio::test]
async fn repeated_handshakes_from_one_address_are_rate_limited() {
    let relay = Relay::start(&[("RELAY_CONN_PER_MIN", "3")]).await;

    let mut accepted = 0;
    let mut refused = 0;
    for _ in 0..6 {
        match connect_async(relay.host_url()).await {
            Ok((socket, _)) => {
                accepted += 1;
                drop(socket);
            }
            Err(WsError::Http(response)) => {
                assert_eq!(response.status().as_u16(), 429, "the refusal is a 429");
                refused += 1;
            }
            Err(other) => panic!("unexpected handshake failure: {other:?}"),
        }
    }

    assert_eq!(accepted, 3, "the configured budget is honoured exactly");
    assert_eq!(refused, 3);
}

#[tokio::test]
async fn a_shutdown_tells_a_connected_device_why() {
    let relay = Relay::with_hosts().await;
    let mut host = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;
    let (mut device, _route) = attach_device(&relay, HOST_ID, &mut host).await;

    relay
        .registry()
        .notify_everyone_of_shutdown("The relay is shutting down.");

    match next_device(&mut device).await {
        DeviceOutbound::Shutdown { reason } => {
            assert!(reason.contains("shutting down"), "got {reason:?}");
        }
        other => panic!("expected a shutdown notice, got {other:?}"),
    }
}

#[tokio::test]
async fn the_health_check_counts_without_identifying() {
    let relay = Relay::with_hosts().await;

    let (status, body) = http_get(&relay.health_url()).await;
    assert_eq!(status, 200);
    assert!(body.contains(r#""status":"ok""#), "got {body}");
    assert!(body.contains(r#""hosts":0"#), "got {body}");
    assert!(body.contains(r#""devices":0"#), "got {body}");
    assert!(
        !body.contains(HOST_ID),
        "a health check must not become an inventory: {body}"
    );

    let mut host = register_host(&relay, HOST_ID, HOST_CREDENTIAL).await;
    let _device = attach_device(&relay, HOST_ID, &mut host).await;

    let (status, body) = http_get(&relay.health_url()).await;
    assert_eq!(status, 200);
    assert!(body.contains(r#""hosts":1"#), "got {body}");
    assert!(body.contains(r#""devices":1"#), "got {body}");
    assert!(!body.contains(HOST_ID), "got {body}");
}

#[tokio::test]
async fn an_unknown_path_is_a_not_found_rather_than_a_relay_error() {
    let relay = Relay::with_hosts().await;
    let (status, _body) = http_get(&format!("http://{}/ws/nothing", relay.address)).await;
    assert_eq!(status, 404);
}
