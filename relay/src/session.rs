//! The two WebSocket legs: one from a Windows host, one from a remote device.
//!
//! Each leg is a small state machine with a strict opening:
//!
//! 1. the first frame must be a `hello` within [`HELLO_TIMEOUT`], otherwise the
//!    socket is dropped without a reply;
//! 2. a host must additionally present the credential the relay was configured
//!    with, compared in constant time;
//! 3. only then is the socket entered into the routing table.
//!
//! Everything after that is opaque. The relay moves `payload` objects from one
//! connection to another and never looks inside them, so there is no code path
//! here that could act on a timer command or mint a device credential.
//!
//! When a leg ends — for any reason, including a process-wide shutdown — the
//! socket is removed from the routing table and the far end is told. Nothing is
//! written to disk at any point.

use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::sink::SinkExt;
use futures_util::stream::{SplitSink, SplitStream, StreamExt};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::http::AppState;
use crate::logging;
use crate::registry::{ForwardRefused, HostRefused, OpenRefused, OUTBOX};
use crate::wire::{
    self, codes, DeviceInbound, DeviceOutbound, HostInbound, HostOutbound, Peer,
};

/// How long a freshly accepted socket may stay silent before its first envelope.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How long an established socket may stay silent before the relay gives up on
/// it. Conforming clients answer the relay's pings automatically, so a healthy
/// but idle phone never hits this.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// How often the relay pings an established socket.
pub const PING_INTERVAL: Duration = Duration::from_secs(30);

/// A host credential comparison that does not leak how much of it matched.
///
/// Both sides are hashed first, so the comparison is over two equal-length
/// digests and a partly-correct credential takes the same time as a wholly
/// wrong one.
pub fn credential_matches(expected: &str, presented: &str) -> bool {
    let expected = Sha256::digest(expected.as_bytes());
    let presented = Sha256::digest(presented.as_bytes());
    expected
        .iter()
        .zip(presented.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

// ---------------------------------------------------------------------------
// Host leg
// ---------------------------------------------------------------------------

/// What the host loop should do after one wake-up.
enum HostStep {
    /// Nothing to write; keep reading.
    Nothing,
    /// Send a WebSocket ping.
    Ping,
    /// Write this envelope and keep going.
    Send(HostOutbound),
    /// Stop; the socket or the far end is finished.
    Stop,
}

/// Serves one host socket from hello to disconnect.
pub async fn host_session(
    socket: WebSocket,
    state: AppState,
    address: String,
    user_agent: Option<String>,
) {
    let (mut sink, mut stream) = socket.split();

    let (host_id, host_token) = match read_hello::<HostInbound>(&mut stream).await {
        Hello::Envelope(HostInbound::Hello {
            protocol,
            host_id,
            host_token,
        }) => {
            if protocol != wire::RELAY_PROTOCOL {
                let _ = send(
                    &mut sink,
                    &HostOutbound::error(
                        codes::UNSUPPORTED_PROTOCOL,
                        format!(
                            "This relay speaks relay/{}; the host announced {protocol}.",
                            wire::RELAY_PROTOCOL
                        ),
                    ),
                )
                .await;
                return;
            }
            (host_id, host_token)
        }
        Hello::Envelope(_) => {
            let _ = send(
                &mut sink,
                &HostOutbound::error(codes::MALFORMED, "The first host envelope must be a hello."),
            )
            .await;
            return;
        }
        Hello::Malformed => {
            let _ = send(
                &mut sink,
                &HostOutbound::error(
                    codes::MALFORMED,
                    "The first host envelope was not a valid relay envelope.",
                ),
            )
            .await;
            return;
        }
        Hello::Silent => {
            logging::warn("host-hello-timeout", &[("peer", address.clone())]);
            return;
        }
        Hello::Gone => return,
    };

    // A malformed id and an unknown id get the same answer: the relay does not
    // confirm which host identifiers exist.
    let authorized = wire::valid_host_id(&host_id)
        && state
            .config
            .host_credential(&host_id)
            .is_some_and(|expected| credential_matches(expected, &host_token));

    if !authorized {
        if !note_failed_registration(&state, &address) {
            let _ = send(
                &mut sink,
                &HostOutbound::error(
                    codes::RATE_LIMITED,
                    "Too many failed registrations from this address.",
                ),
            )
            .await;
            logging::warn("host-registration-limited", &[("peer", address)]);
            return;
        }
        logging::warn(
            "host-registration-refused",
            &[("peer", address.clone()), ("host", host_id.clone())],
        );
        let _ = send(
            &mut sink,
            &HostOutbound::error(
                codes::UNAUTHORIZED_HOST,
                "This relay does not recognise that host identifier and credential.",
            ),
        )
        .await;
        return;
    }

    let (outbound_tx, mut outbound_rx) = mpsc::channel::<HostOutbound>(OUTBOX);
    match state.registry.register_host(&host_id, outbound_tx) {
        Ok(()) => {}
        Err(HostRefused::Conflict) => {
            let _ = send(
                &mut sink,
                &HostOutbound::error(
                    codes::HOST_CONFLICT,
                    "That host identifier is already connected to this relay.",
                ),
            )
            .await;
            logging::warn("host-conflict", &[("host", host_id)]);
            return;
        }
        Err(HostRefused::Full) => {
            let _ = send(
                &mut sink,
                &HostOutbound::error(
                    codes::LIMIT_EXCEEDED,
                    "The relay is tracking as many hosts as it is configured to.",
                ),
            )
            .await;
            logging::warn("host-limit-reached", &[]);
            return;
        }
    }

    // A good credential clears the failed-attempt history for this address, so
    // an operator who mistyped once is not left throttled.
    state.limits.lock().unwrap().registrations.forget(&address);

    if !send(
        &mut sink,
        &HostOutbound::Ready {
            protocol: wire::RELAY_PROTOCOL,
            host_id: host_id.clone(),
        },
    )
    .await
    {
        state.registry.unregister_host(&host_id);
        return;
    }

    logging::info(
        "host-online",
        &[
            ("host", host_id.clone()),
            ("peer", address.clone()),
            ("agent", user_agent.unwrap_or_else(|| "-".to_string())),
        ],
    );

    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await; // the first tick is immediate; nothing is due yet
    let mut last_seen = Instant::now();

    loop {
        let step = tokio::select! {
            incoming = stream.next() => {
                last_seen = Instant::now();
                match incoming {
                    Some(Ok(Message::Text(text))) => host_step(&state, &host_id, &text),
                    Some(Ok(Message::Binary(_))) => HostStep::Send(HostOutbound::error(
                        codes::MALFORMED,
                        "The relay only accepts text envelopes.",
                    )),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => HostStep::Nothing,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => HostStep::Stop,
                }
            }
            message = outbound_rx.recv() => match message {
                Some(message) => HostStep::Send(message),
                // The routing table dropped this host (for example on shutdown).
                None => HostStep::Stop,
            },
            _ = ping.tick() => {
                if last_seen.elapsed() > IDLE_TIMEOUT {
                    HostStep::Stop
                } else {
                    HostStep::Ping
                }
            }
        };

        let keep_going = match step {
            HostStep::Stop => break,
            HostStep::Nothing => true,
            HostStep::Ping => sink.send(Message::Ping(Vec::<u8>::new().into())).await.is_ok(),
            HostStep::Send(message) => send(&mut sink, &message).await,
        };
        if !keep_going {
            break;
        }
    }

    let closed = state.registry.unregister_host(&host_id);
    logging::info(
        "host-offline",
        &[
            ("host", host_id),
            ("peer", address),
            ("routesClosed", closed.len().to_string()),
        ],
    );
    let _ = sink.send(Message::Close(None)).await;
}

/// Interprets one host frame. Pure apart from the routing table and the
/// limiter, both of which are touched without awaiting.
fn host_step(state: &AppState, host_id: &str, text: &str) -> HostStep {
    match wire::parse::<HostInbound>(text) {
        Ok(HostInbound::Data { route, payload }) => {
            if !wire::valid_route_id(&route) {
                logging::debug("host-frame-route-invalid", &[("host", host_id.to_string())]);
                return HostStep::Nothing;
            }
            if !wire::payload_within(&payload, state.config.max_payload_bytes) {
                return HostStep::Send(HostOutbound::error(
                    codes::TOO_LARGE,
                    "That payload is larger than the relay forwards.",
                ));
            }
            match state.registry.forward_to_device(&route, payload) {
                Ok(()) => HostStep::Nothing,
                // Telling the host the route is dead stops it sending again.
                Err(ForwardRefused::UnknownRoute) => HostStep::Send(HostOutbound::Closed {
                    route,
                    reason: "unknown-route".to_string(),
                }),
                Err(ForwardRefused::Gone) => HostStep::Send(HostOutbound::Closed {
                    route,
                    reason: "peer-gone".to_string(),
                }),
                Err(ForwardRefused::Slow) => HostStep::Send(HostOutbound::error(
                    codes::RATE_LIMITED,
                    "That device is not reading fast enough; the frame was dropped.",
                )),
            }
        }
        Ok(HostInbound::Close { route, reason }) => {
            state
                .registry
                .close_route(&route, reason.as_deref().unwrap_or("host-closed"), false);
            HostStep::Nothing
        }
        Ok(HostInbound::Hello { .. }) => HostStep::Send(HostOutbound::error(
            codes::MALFORMED,
            "The hello envelope must be the first frame.",
        )),
        Err(_) => HostStep::Send(HostOutbound::error(
            codes::MALFORMED,
            "That message was not a valid relay envelope.",
        )),
    }
}

// ---------------------------------------------------------------------------
// Device leg
// ---------------------------------------------------------------------------

/// What the device loop should do after one wake-up.
enum DeviceStep {
    Nothing,
    Ping,
    Send(DeviceOutbound),
    /// Tell the device why, then close: the route is not coming back.
    SendThenStop(DeviceOutbound),
    Stop,
}

/// Serves one remote device socket from hello to disconnect.
pub async fn device_session(
    socket: WebSocket,
    state: AppState,
    host_id: String,
    address: String,
    user_agent: Option<String>,
) {
    let (mut sink, mut stream) = socket.split();

    let hello = read_hello::<DeviceInbound>(&mut stream).await;
    match hello {
        Hello::Envelope(DeviceInbound::Hello { protocol }) => {
            if protocol != wire::RELAY_PROTOCOL {
                let _ = send(
                    &mut sink,
                    &DeviceOutbound::error(
                        codes::UNSUPPORTED_PROTOCOL,
                        format!(
                            "This relay speaks relay/{}; the device announced {protocol}.",
                            wire::RELAY_PROTOCOL
                        ),
                    ),
                )
                .await;
                return;
            }
        }
        Hello::Envelope(_) => {
            let _ = send(
                &mut sink,
                &DeviceOutbound::error(codes::MALFORMED, "The first device envelope must be a hello."),
            )
            .await;
            return;
        }
        Hello::Malformed => {
            let _ = send(
                &mut sink,
                &DeviceOutbound::error(
                    codes::MALFORMED,
                    "The first device envelope was not a valid relay envelope.",
                ),
            )
            .await;
            return;
        }
        Hello::Silent => {
            logging::warn("device-hello-timeout", &[("peer", address)]);
            return;
        }
        Hello::Gone => return,
    }

    // Knowing the host id gets a device nothing: it still has to be paired with
    // the host, over this route, before any command is accepted there.
    if !state.config.knows_host(&host_id) {
        logging::warn("device-unknown-host", &[("host", host_id.clone()), ("peer", address)]);
        let _ = send(
            &mut sink,
            &DeviceOutbound::error(
                codes::UNKNOWN_HOST,
                "This relay does not serve that host identifier.",
            ),
        )
        .await;
        return;
    }

    let (device_tx, mut device_rx) = mpsc::channel::<DeviceOutbound>(OUTBOX);
    let route = match state.registry.open_route(
        &host_id,
        device_tx,
        Peer {
            address: address.clone(),
        },
        user_agent,
    ) {
        Ok(route) => route,
        Err(refused) => {
            let (code, message) = match refused {
                OpenRefused::HostOffline => (
                    codes::HOST_OFFLINE,
                    "The host is not connected to the relay at the moment. Try again shortly.",
                ),
                OpenRefused::Limit => (
                    codes::LIMIT_EXCEEDED,
                    "The host already has as many devices attached as the relay allows.",
                ),
                OpenRefused::Slow => (
                    codes::LIMIT_EXCEEDED,
                    "The host is not keeping up with its connected devices.",
                ),
                OpenRefused::Internal => (
                    codes::INTERNAL,
                    "The relay could not allocate a route for this connection.",
                ),
            };
            let _ = send(&mut sink, &DeviceOutbound::error(code, message)).await;
            logging::warn(
                "device-route-refused",
                &[("host", host_id), ("peer", address), ("reason", code.to_string())],
            );
            return;
        }
    };

    logging::info(
        "device-attached",
        &[
            ("host", host_id.clone()),
            ("route", logging::route_hint(&route)),
            ("peer", address.clone()),
        ],
    );

    if !send(
        &mut sink,
        &DeviceOutbound::Ready {
            protocol: wire::RELAY_PROTOCOL,
            route: route.clone(),
            host_id: host_id.clone(),
            host_online: true,
        },
    )
    .await
    {
        state.registry.close_route(&route, "peer-gone", true);
        return;
    }

    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    let mut last_seen = Instant::now();
    let mut stop_reason = "peer-gone";

    loop {
        let step = tokio::select! {
            incoming = stream.next() => {
                last_seen = Instant::now();
                match incoming {
                    Some(Ok(Message::Text(text))) => device_step(&state, &route, &text),
                    Some(Ok(Message::Binary(_))) => DeviceStep::Send(DeviceOutbound::error(
                        codes::MALFORMED,
                        "The relay only accepts text envelopes.",
                    )),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => DeviceStep::Nothing,
                    Some(Ok(Message::Close(_))) => DeviceStep::Stop,
                    Some(Err(_)) | None => DeviceStep::Stop,
                }
            }
            message = device_rx.recv() => match message {
                Some(DeviceOutbound::Shutdown { reason }) => {
                    stop_reason = "host-offline";
                    DeviceStep::SendThenStop(DeviceOutbound::Shutdown { reason })
                }
                Some(message) => DeviceStep::Send(message),
                None => DeviceStep::Stop,
            },
            _ = ping.tick() => {
                if last_seen.elapsed() > IDLE_TIMEOUT {
                    stop_reason = "idle";
                    DeviceStep::Stop
                } else {
                    DeviceStep::Ping
                }
            }
        };

        match step {
            DeviceStep::Stop => break,
            DeviceStep::Nothing => {}
            DeviceStep::Ping => {
                if sink.send(Message::Ping(Vec::<u8>::new().into())).await.is_err() {
                    break;
                }
            }
            DeviceStep::Send(message) => {
                if !send(&mut sink, &message).await {
                    break;
                }
            }
            DeviceStep::SendThenStop(message) => {
                let _ = send(&mut sink, &message).await;
                break;
            }
        }
    }

    state.registry.close_route(&route, stop_reason, true);
    logging::info(
        "device-detached",
        &[
            ("host", host_id),
            ("route", logging::route_hint(&route)),
            ("reason", stop_reason.to_string()),
        ],
    );
    let _ = sink.send(Message::Close(None)).await;
}

/// Interprets one device frame.
fn device_step(state: &AppState, route: &str, text: &str) -> DeviceStep {
    match wire::parse::<DeviceInbound>(text) {
        Ok(DeviceInbound::Data { payload }) => {
            if !allow_frame(state, route) {
                return DeviceStep::Send(DeviceOutbound::error(
                    codes::RATE_LIMITED,
                    "Too many messages; slow down.",
                ));
            }
            if !wire::payload_within(&payload, state.config.max_payload_bytes) {
                return DeviceStep::Send(DeviceOutbound::error(
                    codes::TOO_LARGE,
                    "That payload is larger than the relay forwards.",
                ));
            }
            match state.registry.forward_to_host(route, payload) {
                Ok(()) => DeviceStep::Nothing,
                Err(ForwardRefused::UnknownRoute) | Err(ForwardRefused::Gone) => {
                    DeviceStep::SendThenStop(DeviceOutbound::error(
                        codes::HOST_OFFLINE,
                        "The host connection has gone away.",
                    ))
                }
                Err(ForwardRefused::Slow) => DeviceStep::Send(DeviceOutbound::error(
                    codes::RATE_LIMITED,
                    "The host is not reading fast enough; the frame was dropped.",
                )),
            }
        }
        Ok(DeviceInbound::Hello { .. }) => DeviceStep::Send(DeviceOutbound::error(
            codes::MALFORMED,
            "The hello envelope must be the first frame.",
        )),
        Err(_) => DeviceStep::Send(DeviceOutbound::error(
            codes::MALFORMED,
            "That message was not a valid relay envelope.",
        )),
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// The outcome of waiting for the first frame.
enum Hello<T> {
    Envelope(T),
    /// The first frame was not an envelope. The reason is deliberately not
    /// carried: it would come from the parser, and a parse failure on a hello
    /// can quote the value that failed to parse — which is where the host
    /// credential travels.
    Malformed,
    /// The socket stayed silent past the deadline.
    Silent,
    /// The socket closed or failed.
    Gone,
}

async fn read_hello<T>(stream: &mut SplitStream<WebSocket>) -> Hello<T>
where
    T: serde::de::DeserializeOwned,
{
    match tokio::time::timeout(HELLO_TIMEOUT, stream.next()).await {
        Err(_) => Hello::Silent,
        Ok(None) | Ok(Some(Err(_))) => Hello::Gone,
        Ok(Some(Ok(Message::Text(text)))) => match wire::parse::<T>(&text) {
            Ok(envelope) => Hello::Envelope(envelope),
            Err(_) => Hello::Malformed,
        },
        Ok(Some(Ok(_))) => Hello::Malformed,
    }
}

/// Writes one envelope. Returns false when the socket is no longer writable.
async fn send<T: Serialize>(sink: &mut SplitSink<WebSocket, Message>, message: &T) -> bool {
    sink.send(Message::text(wire::to_text(message)))
        .await
        .is_ok()
}

/// Records a refused host registration, reporting whether the address may keep
/// trying. The guard is scoped so it is never held across an await.
fn note_failed_registration(state: &AppState, address: &str) -> bool {
    state
        .limits
        .lock()
        .unwrap()
        .registrations
        .check(address, logging::now_ms())
}

/// Records one forwarded frame against a route's budget.
fn allow_frame(state: &AppState, route: &str) -> bool {
    state
        .limits
        .lock()
        .unwrap()
        .frames
        .check(route, logging::now_ms())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;
    use crate::config::Config;
    use crate::limits::Limits;
    use crate::registry::Registry;

    fn state(max_payload: usize) -> AppState {
        let mut vars = std::collections::BTreeMap::new();
        vars.insert("RELAY_TLS_CERT".to_string(), "/c.pem".to_string());
        vars.insert("RELAY_TLS_KEY".to_string(), "/k.pem".to_string());
        vars.insert("RELAY_HOSTS".to_string(), "h-abcdefgh:correct-horse-battery".to_string());
        let config = Config::parse(vars).unwrap();

        AppState {
            config: Arc::new(Config {
                max_payload_bytes: max_payload,
                ..config
            }),
            registry: Arc::new(Registry::new(4, 4)),
            limits: Arc::new(Mutex::new(Limits::new(10, 60))),
        }
    }

    #[test]
    fn a_credential_matches_only_itself() {
        assert!(credential_matches("correct-horse-battery", "correct-horse-battery"));
        assert!(!credential_matches("correct-horse-battery", "correct-horse-batterz"));
        assert!(!credential_matches("correct-horse-battery", "correct-horse-batter"));
        assert!(!credential_matches("correct-horse-battery", ""));
        assert!(!credential_matches("", "correct-horse-battery"));
    }

    #[test]
    fn a_payload_for_an_unknown_route_is_refused_without_reaching_anything() {
        let state = state(1024);
        let step = device_step(&state, "not-a-route", r#"{"t":"data","payload":{"type":"ping"}}"#);
        assert!(matches!(step, DeviceStep::SendThenStop(_)));
    }

    #[test]
    fn an_oversized_device_payload_is_refused_before_routing() {
        let state = state(16);
        let (tx, _rx) = mpsc::channel(OUTBOX);
        state.registry.register_host("h-abcdefgh", tx).unwrap();
        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        let route = state
            .registry
            .open_route(
                "h-abcdefgh",
                device_tx,
                Peer { address: "203.0.113.9:1".to_string() },
                None,
            )
            .unwrap();

        let big = json!({"type":"command","id":"c","payload":"x".repeat(200)});
        let text = format!(r#"{{"t":"data","payload":{big}}}"#);
        assert!(matches!(device_step(&state, &route, &text), DeviceStep::Send(_)));
    }

    #[test]
    fn a_device_frame_is_forwarded_to_its_host_untouched() {
        let state = state(4096);
        let (tx, mut rx) = mpsc::channel(OUTBOX);
        state.registry.register_host("h-abcdefgh", tx).unwrap();
        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        let route = state
            .registry
            .open_route(
                "h-abcdefgh",
                device_tx,
                Peer { address: "203.0.113.9:1".to_string() },
                None,
            )
            .unwrap();
        let _ = rx.try_recv().unwrap(); // the open

        let text = r#"{"t":"data","payload":{"type":"command","id":"c-1","revision":7}}"#;
        assert!(matches!(device_step(&state, &route, text), DeviceStep::Nothing));

        match rx.try_recv().unwrap() {
            HostOutbound::Data { route: carried, payload } => {
                assert_eq!(carried, route);
                assert_eq!(payload["revision"], json!(7));
            }
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn a_host_frame_for_an_unknown_route_tells_the_host_to_drop_it() {
        let state = state(4096);
        let text = r#"{"t":"data","route":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","payload":{"type":"state"}}"#;
        assert!(matches!(
            host_step(&state, "h-abcdefgh", text),
            HostStep::Send(HostOutbound::Closed { .. })
        ));
    }

    #[test]
    fn a_syntactically_bad_route_is_dropped_without_a_reply() {
        let state = state(4096);
        let text = r#"{"t":"data","route":"short","payload":{"type":"state"}}"#;
        assert!(matches!(host_step(&state, "h-abcdefgh", text), HostStep::Nothing));
    }

    #[test]
    fn a_non_envelope_frame_is_answered_with_malformed() {
        let state = state(4096);
        assert!(matches!(
            device_step(&state, "route", "not json at all"),
            DeviceStep::Send(_)
        ));
        assert!(matches!(
            host_step(&state, "h-abcdefgh", "not json at all"),
            HostStep::Send(_)
        ));
    }

    #[test]
    fn the_frame_limiter_applies_per_route() {
        let state = state(4096); // sixty frames per second, per route
        let text = r#"{"t":"data","payload":{"type":"ping"}}"#;

        let mut limited = 0;
        for _ in 0..80 {
            if let DeviceStep::Send(DeviceOutbound::Error { code, .. }) =
                device_step(&state, "route", text)
            {
                assert_eq!(code, codes::RATE_LIMITED);
                limited += 1;
            }
        }
        assert!(limited > 0, "a route sending 80 frames at once must be limited");

        // A route that has not been hammered still has its whole budget: the
        // only reason this one is refused is that the route is unknown, not
        // that it is out of frames.
        assert!(matches!(
            device_step(&state, "another-route", text),
            DeviceStep::SendThenStop(_)
        ));
    }
}
