//! The relay tunnel: one supervisor thread, a route table and a reconnect loop.
//!
//! The tunnel owns exactly one outbound socket to the relay and turns the
//! relay's `relay/1` envelope ([`super::envelope`]) into a stream of events for
//! the layer above. It is deliberately dumb about meaning: a `data` payload goes
//! up untouched, and an outbound payload goes down untouched. The E2E session
//! layer (HPKE pairing, Noise `KK`) sits at this boundary —
//! `m7-e2e-security-design.md` §12.1 — so everything the relay ever sees is
//! already sealed by the time it reaches [`Tunnel::send_data`].
//!
//! The route table maps a relay `route` to the peer the relay reported. It is
//! the tunnel's only piece of state that outlives a single connection, and it is
//! cleared whenever the connection is lost so the layer above can tear the
//! corresponding virtual connections down.
//!
//! # One socket, one thread
//!
//! The design mandates a blocking `rustls` stream
//! (`m7-online-networking.md` §3.2). A single blocking socket cannot wait for
//! inbound bytes and flush outbound ones at the same time, so the socket carries
//! a short read timeout ([`client::IDLE_POLL`]) and every idle tick flushes the
//! outbound queue through [`client::RelayConnection::read_frame_with`]. The frame
//! parser is never restarted, so no partially-read frame is ever lost.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::client::{self, RelayConnection, RelayUrl};
use super::envelope::{self, HostInbound, HostOutbound};
use super::super::transport::{self, Frame};

/// First reconnect delay; doubles per consecutive failure.
const BACKOFF_BASE_MS: u64 = 500;

/// Longest reconnect delay. Matches the remote client's bound so the two ends
/// of the product back off alike.
const BACKOFF_MAX_MS: u64 = 15_000;

/// How many consecutive failures may double the delay before it is capped.
const BACKOFF_CAP_STEPS: u32 = 5;

// ---------------------------------------------------------------------------
// Events and status
// ---------------------------------------------------------------------------

/// Where the tunnel currently stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TunnelState {
    /// Not started, or stopped.
    Stopped,
    /// Trying to reach the relay.
    Connecting,
    /// The WebSocket to the relay is up; the host hello may still be in flight.
    Online,
    /// The connection dropped; the supervisor is waiting to retry.
    Offline,
}

/// Something the tunnel observed, handed to [`TunnelSink::event`].
///
/// Borrowed on purpose: an event is dispatched once, on the supervisor thread,
/// and never stored.
#[derive(Debug)]
pub enum TunnelEvent<'a> {
    /// The supervisor started a connection attempt.
    Connecting,
    /// The WebSocket to the relay came up.
    Online,
    /// The connection dropped or could not be made.
    Offline { reason: &'a str },
    /// The relay acknowledged the host. `protocol` is the relay envelope
    /// version it will speak.
    Ready { protocol: u32, host_id: &'a str },
    /// A device opened a route.
    Open {
        route: &'a str,
        peer: &'a str,
        user_agent: Option<&'a str>,
    },
    /// One sealed frame arrived for a route.
    Data { route: &'a str, payload: &'a Value },
    /// A route ended.
    Closed { route: &'a str, reason: &'a str },
    /// The relay, or the tunnel itself, refused something.
    Rejected { code: &'a str, message: &'a str },
}

/// The single method the layer above implements.
///
/// Implementors must be cheap and must not block: the supervisor calls this
/// inline, and a slow sink stalls the tunnel.
pub trait TunnelSink: Send + Sync + 'static {
    fn event(&self, event: TunnelEvent<'_>);
}

/// What the Settings page and the IPC layer render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStatus {
    pub state: TunnelState,
    /// The relay origin, as configured.
    pub relay: String,
    pub host_id: String,
    /// Live routes, as the relay reported their peers.
    pub routes: Vec<String>,
    pub peers: Vec<String>,
    /// Why the tunnel is not online, if it is not.
    pub last_error: Option<String>,
}

/// Everything the supervisor owns behind one lock.
#[derive(Default)]
struct Shared {
    state: TunnelState,
    last_error: Option<String>,
    /// `route -> peer address`, in insertion order for a stable display.
    routes: Vec<(String, String)>,
}

impl Default for TunnelState {
    fn default() -> Self {
        TunnelState::Stopped
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

// ---------------------------------------------------------------------------
// Tunnel configuration
// ---------------------------------------------------------------------------

/// Everything needed to reach the relay and identify this host.
#[derive(Debug, Clone)]
pub struct TunnelConfig {
    pub url: RelayUrl,
    pub host_id: String,
    pub host_token: String,
}

// ---------------------------------------------------------------------------
// Outbound queue
// ---------------------------------------------------------------------------

/// Frames the layer above wants written to the relay.
///
/// A queue rather than a channel: the supervisor must flush whatever has
/// accumulated each time the read would block, and a `VecDeque` makes "take
/// everything now" trivial and order-preserving.
#[derive(Default)]
struct OutboundQueue {
    items: Mutex<VecDeque<HostInbound>>,
}

impl OutboundQueue {
    fn push(&self, item: HostInbound) {
        lock(&self.items).push_back(item);
    }

    fn is_empty(&self) -> bool {
        lock(&self.items).is_empty()
    }

    /// Writes every queued frame, in order, and clears the queue.
    fn flush(&self, writer: &mut dyn Write) -> io::Result<()> {
        let pending: Vec<HostInbound> = {
            let mut items = lock(&self.items);
            items.drain(..).collect()
        };
        for item in pending {
            client::frame::text(writer, &envelope::to_text(&item))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tunnel handle
// ---------------------------------------------------------------------------

/// The handle the rest of the host holds.
pub struct Tunnel {
    outbound: Arc<OutboundQueue>,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    relay: String,
    host_id: String,
    supervisor: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Tunnel {
    /// Starts the supervisor. It connects immediately and reconnects on its own
    /// until [`Tunnel::stop`] is called.
    pub fn start(config: TunnelConfig, sink: Arc<dyn TunnelSink>) -> Tunnel {
        let outbound = Arc::new(OutboundQueue::default());
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared::default()));

        let handle = {
            let outbound = Arc::clone(&outbound);
            let stop = Arc::clone(&stop);
            let shared = Arc::clone(&shared);
            let config = config.clone();
            thread::spawn(move || supervise(config, sink, outbound, stop, shared))
        };

        Tunnel {
            outbound,
            stop,
            shared,
            relay: config.url.origin(),
            host_id: config.host_id,
            supervisor: Mutex::new(Some(handle)),
        }
    }

    /// Queues one already-sealed frame for a route.
    ///
    /// Returns `false` when the tunnel has been stopped, so the caller can
    /// report the loss rather than assume delivery.
    pub fn send_data(&self, route: &str, payload: Value) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return false;
        }
        self.outbound.push(HostInbound::Data {
            route: route.to_string(),
            payload,
        });
        true
    }

    /// Asks the relay to close one route, so the device's socket is released.
    pub fn close_route(&self, route: &str, reason: Option<&str>) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return false;
        }
        self.outbound.push(HostInbound::Close {
            route: route.to_string(),
            reason: reason.map(str::to_string),
        });
        true
    }

    /// True when there is nothing queued. Used by tests and diagnostics.
    pub fn outbound_is_empty(&self) -> bool {
        self.outbound.is_empty()
    }

    /// The current status.
    pub fn status(&self) -> TunnelStatus {
        let shared = lock(&self.shared);
        TunnelStatus {
            state: shared.state,
            relay: self.relay.clone(),
            host_id: self.host_id.clone(),
            routes: shared.routes.iter().map(|(route, _)| route.clone()).collect(),
            peers: shared.routes.iter().map(|(_, peer)| peer.clone()).collect(),
            last_error: shared.last_error.clone(),
        }
    }

    /// Stops the supervisor and waits for the socket to be released.
    ///
    /// Idempotent: a second call is a no-op.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let handle = lock(&self.supervisor).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// How one connection attempt ended.
enum Outcome {
    /// Asked to stop.
    Stopped,
    /// The connection ended; reconnecting may help.
    Retry(String),
    /// The relay refused in a way that retrying will not fix.
    Refused(String),
}

fn supervise(
    config: TunnelConfig,
    sink: Arc<dyn TunnelSink>,
    outbound: Arc<OutboundQueue>,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
) {
    let mut failures: u32 = 0;

    while !stop.load(Ordering::Relaxed) {
        set_state(&shared, &*sink, TunnelState::Connecting, None);

        let outcome = match RelayConnection::open(&config.url) {
            Ok(link) => {
                // The WebSocket to the relay is up; the host hello goes out
                // inside `run`. Registration proper is confirmed by the relay's
                // own `ready`, which is reported as its own event.
                set_state(&shared, &*sink, TunnelState::Online, None);
                run(link, &config, &*sink, &outbound, &stop, &shared)
            }
            Err(error) => Outcome::Retry(error),
        };

        // Whatever ended the connection, every route it carried is gone. The
        // layer above is told, so no virtual connection survives a dead tunnel.
        let lost = clear_routes(&shared, &*sink);

        match outcome {
            Outcome::Stopped => break,
            Outcome::Refused(reason) => {
                // The refusal itself was reported where it was detected.
                set_state(&shared, &*sink, TunnelState::Offline, Some(&reason));
                sink.event(TunnelEvent::Offline { reason: &reason });
                break;
            }
            Outcome::Retry(reason) => {
                let _ = lost;
                failures = failures.saturating_add(1);
                set_state(&shared, &*sink, TunnelState::Offline, Some(&reason));
                sink.event(TunnelEvent::Offline { reason: &reason });
                sleep_until(stop.as_ref(), backoff(failures));
            }
        }
    }

    set_state(&shared, &*sink, TunnelState::Stopped, None);
}

/// `min(500 · 2ⁿ⁻¹, 15000)` ms, with `n` the number of consecutive failures.
fn backoff(failures: u32) -> Duration {
    let steps = failures.saturating_sub(1).min(BACKOFF_CAP_STEPS);
    let millis = BACKOFF_BASE_MS
        .saturating_mul(1u64 << steps)
        .min(BACKOFF_MAX_MS);
    Duration::from_millis(millis)
}

/// Sleeps in short slices so a stop request is honoured promptly.
fn sleep_until(stop: &AtomicBool, total: Duration) {
    let deadline = std::time::Instant::now() + total;
    while !stop.load(Ordering::Relaxed) {
        let now = std::time::Instant::now();
        if now >= deadline {
            return;
        }
        thread::sleep((deadline - now).min(client::IDLE_POLL));
    }
}

fn set_state(shared: &Mutex<Shared>, sink: &dyn TunnelSink, state: TunnelState, error: Option<&str>) {
    {
        let mut shared = lock(shared);
        shared.state = state;
        shared.last_error = error.map(str::to_string);
    }
    match state {
        TunnelState::Connecting => sink.event(TunnelEvent::Connecting),
        TunnelState::Online => sink.event(TunnelEvent::Online),
        // `Offline` is announced with its reason by the caller.
        TunnelState::Offline | TunnelState::Stopped => {}
    }
}

/// Forgets every route and tells the layer above each one closed.
fn clear_routes(shared: &Mutex<Shared>, sink: &dyn TunnelSink) -> usize {
    let routes: Vec<String> = {
        let mut shared = lock(shared);
        let routes = shared.routes.drain(..).map(|(route, _)| route).collect();
        routes
    };
    for route in &routes {
        sink.event(TunnelEvent::Closed {
            route,
            reason: "tunnel-lost",
        });
    }
    routes.len()
}

fn run(
    mut link: RelayConnection,
    config: &TunnelConfig,
    sink: &dyn TunnelSink,
    outbound: &OutboundQueue,
    stop: &AtomicBool,
    shared: &Mutex<Shared>,
) -> Outcome {
    let hello = HostInbound::Hello {
        protocol: envelope::RELAY_PROTOCOL,
        host_id: config.host_id.clone(),
        host_token: config.host_token.clone(),
    };
    if let Err(error) = link.send_text(&envelope::to_text(&hello)) {
        return Outcome::Retry(format!("The relay did not accept the host hello: {error}"));
    }

    loop {
        let frame = link.read_frame_with(|writer| {
            if stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "stopping"));
            }
            outbound.flush(writer)
        });

        match frame {
            Ok(Frame::Text(text)) => match handle(&text, sink, shared) {
                Step::Continue => {}
                Step::Stop(outcome) => return outcome,
            },
            Ok(Frame::Ping(payload)) => {
                if let Err(error) = link.send_pong(&payload) {
                    return Outcome::Retry(format!("The relay ping could not be answered: {error}"));
                }
            }
            Ok(Frame::Pong(_)) => {}
            Ok(Frame::Close) => {
                return Outcome::Retry("The relay closed the host connection.".to_string());
            }
            Ok(Frame::Binary(_)) => {
                return Outcome::Retry("The relay sent a binary frame; relay/1 is text-only.".to_string());
            }
            Err(error) if stop.load(Ordering::Relaxed) => {
                let _ = error;
                return Outcome::Stopped;
            }
            Err(error) => {
                return Outcome::Retry(format!("The relay connection failed: {error}"));
            }
        }
    }
}

/// Whether the read loop should carry on.
enum Step {
    Continue,
    Stop(Outcome),
}

fn handle(text: &str, sink: &dyn TunnelSink, shared: &Mutex<Shared>) -> Step {
    let message: HostOutbound = match envelope::parse(text) {
        Ok(message) => message,
        Err(error) => {
            let reason = format!("The relay sent an envelope this host does not understand: {error}");
            sink.event(TunnelEvent::Rejected {
                code: envelope::codes::MALFORMED,
                message: &reason,
            });
            return Step::Stop(Outcome::Refused(reason));
        }
    };

    match message {
        HostOutbound::Ready { protocol, host_id } => {
            if protocol != envelope::RELAY_PROTOCOL {
                let reason = format!(
                    "The relay speaks relay/{protocol}; this host speaks relay/{}.",
                    envelope::RELAY_PROTOCOL
                );
                sink.event(TunnelEvent::Rejected {
                    code: envelope::codes::UNSUPPORTED_PROTOCOL,
                    message: &reason,
                });
                return Step::Stop(Outcome::Refused(reason));
            }
            {
                let mut shared = lock(shared);
                shared.routes.clear();
            }
            sink.event(TunnelEvent::Ready {
                protocol,
                host_id: &host_id,
            });
            Step::Continue
        }
        HostOutbound::Open {
            route,
            peer,
            user_agent,
        } => {
            if !envelope::valid_route_id(&route) {
                sink.event(TunnelEvent::Rejected {
                    code: envelope::codes::MALFORMED,
                    message: "The relay opened a route with an invalid identifier.",
                });
                return Step::Continue;
            }
            {
                let mut shared = lock(shared);
                shared.routes.retain(|(existing, _)| existing != &route);
                shared.routes.push((route.clone(), peer.address.clone()));
            }
            sink.event(TunnelEvent::Open {
                route: &route,
                peer: &peer.address,
                user_agent: user_agent.as_deref(),
            });
            Step::Continue
        }
        HostOutbound::Data { route, payload } => {
            if !envelope::payload_within(&payload, envelope::MAX_PAYLOAD_BYTES) {
                sink.event(TunnelEvent::Rejected {
                    code: envelope::codes::TOO_LARGE,
                    message: "The relay delivered a payload the host will not buffer.",
                });
                return Step::Continue;
            }
            let known = lock(shared).routes.iter().any(|(existing, _)| existing == &route);
            if !known {
                // The relay forwarded for a route it never opened, or one it
                // already closed. Either way there is nothing to deliver to.
                sink.event(TunnelEvent::Rejected {
                    code: envelope::codes::UNKNOWN_HOST,
                    message: "The relay delivered data for an unknown route.",
                });
                return Step::Continue;
            }
            sink.event(TunnelEvent::Data {
                route: &route,
                payload: &payload,
            });
            Step::Continue
        }
        HostOutbound::Closed { route, reason } => {
            lock(shared).routes.retain(|(existing, _)| existing != &route);
            sink.event(TunnelEvent::Closed {
                route: &route,
                reason: &reason,
            });
            Step::Continue
        }
        HostOutbound::Error { code, message } => {
            sink.event(TunnelEvent::Rejected {
                code: &code,
                message: &message,
            });
            let fatal = matches!(
                code.as_str(),
                envelope::codes::UNSUPPORTED_PROTOCOL
                    | envelope::codes::UNAUTHORIZED_HOST
                    | envelope::codes::HOST_CONFLICT
            );
            if fatal {
                Step::Stop(Outcome::Refused(message))
            } else {
                Step::Stop(Outcome::Retry(message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc::{Receiver, Sender};

    use serde_json::json;

    /// An owned copy of [`TunnelEvent`], so a test can assert after the borrow.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Seen {
        Connecting,
        Online,
        Offline(String),
        Ready(u32, String),
        Open(String, String),
        Data(String, Value),
        Closed(String, String),
        Rejected(String, String),
    }

    struct Recorder {
        sender: Sender<Seen>,
    }

    impl TunnelSink for Recorder {
        fn event(&self, event: TunnelEvent<'_>) {
            let seen = match event {
                TunnelEvent::Connecting => Seen::Connecting,
                TunnelEvent::Online => Seen::Online,
                TunnelEvent::Offline { reason } => Seen::Offline(reason.to_string()),
                TunnelEvent::Ready { protocol, host_id } => {
                    Seen::Ready(protocol, host_id.to_string())
                }
                TunnelEvent::Open { route, peer, .. } => {
                    Seen::Open(route.to_string(), peer.to_string())
                }
                TunnelEvent::Data { route, payload } => {
                    Seen::Data(route.to_string(), payload.clone())
                }
                TunnelEvent::Closed { route, reason } => {
                    Seen::Closed(route.to_string(), reason.to_string())
                }
                TunnelEvent::Rejected { code, message } => {
                    Seen::Rejected(code.to_string(), message.to_string())
                }
            };
            let _ = self.sender.send(seen);
        }
    }

    fn recorder() -> (Arc<Recorder>, Receiver<Seen>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (Arc::new(Recorder { sender }), receiver)
    }

    /// Waits for the next event, failing the test rather than hanging.
    fn next(receiver: &Receiver<Seen>) -> Seen {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("the tunnel produced no event in time")
    }

    const ROUTE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn assert_route() {
        assert_eq!(ROUTE.len(), envelope::ROUTE_ID_LEN);
        assert!(envelope::valid_route_id(ROUTE));
    }

    /// A stand-in relay: performs the server half of the WebSocket handshake
    /// (reusing the M6 helpers) and then runs `script`.
    fn fake_relay<F>(script: F) -> (u16, thread::JoinHandle<()>)
    where
        F: FnOnce(&mut std::net::TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the fake relay binds");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the fake relay accepts");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("the fake relay sets a read timeout");

            let request = transport::read_request(&mut stream).expect("a request head");
            assert!(request.is_websocket_upgrade(), "the host must upgrade");
            let key = request.websocket_key().expect("a nonce").to_string();
            transport::write_websocket_handshake(&mut stream, &key).expect("the 101 response");

            script(&mut stream);
        });
        (port, handle)
    }

    fn read_text(stream: &mut std::net::TcpStream) -> String {
        match transport::read_frame(stream, 128 * 1024).expect("a frame") {
            Frame::Text(text) => text,
            other => panic!("expected a text frame, got {other:?}"),
        }
    }

    fn write_envelope(stream: &mut std::net::TcpStream, message: &HostOutbound) {
        transport::write_text(stream, &envelope::to_text(message)).expect("the frame is written");
    }

    fn config(port: u16) -> TunnelConfig {
        TunnelConfig {
            url: RelayUrl::parse(&format!("ws://127.0.0.1:{port}")).unwrap(),
            host_id: "h-abcdefgh".to_string(),
            host_token: "test-credential".to_string(),
        }
    }

    fn assert_hello(text: &str) {
        match envelope::parse::<HostInbound>(text).expect("a host envelope") {
            HostInbound::Hello {
                protocol,
                host_id,
                host_token,
            } => {
                assert_eq!(protocol, envelope::RELAY_PROTOCOL);
                assert_eq!(host_id, "h-abcdefgh");
                assert_eq!(host_token, "test-credential");
            }
            other => panic!("expected a hello, got {other:?}"),
        }
    }

    #[test]
    fn the_backoff_is_bounded_exponential() {
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_millis(1000));
        assert_eq!(backoff(3), Duration::from_millis(2000));
        assert_eq!(backoff(4), Duration::from_millis(4000));
        assert_eq!(backoff(5), Duration::from_millis(8000));
        assert_eq!(backoff(6), Duration::from_millis(15_000));
        assert_eq!(backoff(50), Duration::from_millis(15_000));
    }

    #[test]
    fn the_tunnel_registers_reports_routes_and_carries_data() {
        assert_route();
        // Lets the test hold the relay open until it has read the status.
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (port, relay) = fake_relay(move |stream| {
            assert_hello(&read_text(stream));

            write_envelope(
                stream,
                &HostOutbound::Ready {
                    protocol: envelope::RELAY_PROTOCOL,
                    host_id: "h-abcdefgh".to_string(),
                },
            );
            write_envelope(
                stream,
                &HostOutbound::Open {
                    route: ROUTE.to_string(),
                    peer: envelope::Peer {
                        address: "203.0.113.9".to_string(),
                    },
                    user_agent: Some("Mozilla/5.0 (iPhone)".to_string()),
                },
            );
            write_envelope(
                stream,
                &HostOutbound::Data {
                    route: ROUTE.to_string(),
                    payload: json!({"e2e": "m", "v": 1, "ct": "AAAA"}),
                },
            );

            // The host's own frame must arrive, masked and well formed.
            let outbound = read_text(stream);
            match envelope::parse::<HostInbound>(&outbound).expect("a host envelope") {
                HostInbound::Data { route, payload } => {
                    assert_eq!(route, ROUTE);
                    assert_eq!(payload["e2e"], json!("pair"));
                }
                other => panic!("expected data, got {other:?}"),
            }

            // Stay connected until the test releases us.
            let _ = held.recv_timeout(Duration::from_secs(5));
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        assert_eq!(next(&events), Seen::Connecting);
        assert_eq!(next(&events), Seen::Online);
        assert_eq!(
            next(&events),
            Seen::Ready(envelope::RELAY_PROTOCOL, "h-abcdefgh".to_string())
        );
        assert_eq!(
            next(&events),
            Seen::Open(ROUTE.to_string(), "203.0.113.9".to_string())
        );
        assert_eq!(
            next(&events),
            Seen::Data(ROUTE.to_string(), json!({"e2e": "m", "v": 1, "ct": "AAAA"}))
        );

        assert!(tunnel.send_data(ROUTE, json!({"e2e": "pair", "v": 1})));

        let status = tunnel.status();
        assert_eq!(status.state, TunnelState::Online);
        assert_eq!(status.routes, vec![ROUTE.to_string()]);
        assert_eq!(status.peers, vec!["203.0.113.9".to_string()]);

        let _ = release.send(());
        relay.join().expect("the fake relay finishes");

        tunnel.stop();
        assert_eq!(tunnel.status().state, TunnelState::Stopped);
    }

    #[test]
    fn losing_the_relay_closes_every_live_route() {
        let (port, relay) = fake_relay(|stream| {
            assert_hello(&read_text(stream));
            write_envelope(
                stream,
                &HostOutbound::Ready {
                    protocol: envelope::RELAY_PROTOCOL,
                    host_id: "h-abcdefgh".to_string(),
                },
            );
            write_envelope(
                stream,
                &HostOutbound::Open {
                    route: ROUTE.to_string(),
                    peer: envelope::Peer {
                        address: "203.0.113.9".to_string(),
                    },
                    user_agent: None,
                },
            );
            // Drop the connection without a close frame.
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        let mut closed = None;
        for _ in 0..8 {
            match next(&events) {
                Seen::Closed(route, reason) => {
                    closed = Some((route, reason));
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(
            closed,
            Some((ROUTE.to_string(), "tunnel-lost".to_string())),
            "a lost tunnel must close its routes"
        );

        tunnel.stop();
        relay.join().ok();
    }

    #[test]
    fn a_relay_error_code_is_reported_and_an_unauthorized_host_stops_retrying() {
        let (port, relay) = fake_relay(|stream| {
            assert_hello(&read_text(stream));
            write_envelope(
                stream,
                &HostOutbound::error(
                    envelope::codes::UNAUTHORIZED_HOST,
                    "The host credential is not valid.",
                ),
            );
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        let mut saw_rejection = false;
        let mut saw_offline = false;
        for _ in 0..8 {
            match next(&events) {
                Seen::Rejected(code, _) => {
                    assert_eq!(code, envelope::codes::UNAUTHORIZED_HOST);
                    saw_rejection = true;
                }
                Seen::Offline(_) => saw_offline = true,
                _ => {}
            }
            if saw_rejection && saw_offline {
                break;
            }
        }
        assert!(saw_rejection, "the relay error must be reported");
        assert!(saw_offline, "the tunnel must leave the online state");

        tunnel.stop();
        relay.join().ok();
    }

    #[test]
    fn a_relay_version_mismatch_is_refused_rather_than_adapted_to() {
        let (port, relay) = fake_relay(|stream| {
            assert_hello(&read_text(stream));
            write_envelope(
                stream,
                &HostOutbound::Ready {
                    protocol: envelope::RELAY_PROTOCOL + 1,
                    host_id: "h-abcdefgh".to_string(),
                },
            );
            // Nothing else: the host is expected to close.
            let _ = read_text(stream);
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        // Connecting, then Online, then the mismatch rejection.
        let mut refusal = None;
        for _ in 0..8 {
            if let Seen::Rejected(_, message) = next(&events) {
                refusal = Some(message);
                break;
            }
        }
        let refusal = refusal.expect("a version mismatch must be reported");
        assert!(refusal.contains("relay/2"), "{refusal}");

        tunnel.stop();
        relay.join().ok();
    }

    #[test]
    fn data_for_a_route_that_was_never_opened_is_dropped() {
        let (port, relay) = fake_relay(|stream| {
            assert_hello(&read_text(stream));
            write_envelope(
                stream,
                &HostOutbound::Ready {
                    protocol: envelope::RELAY_PROTOCOL,
                    host_id: "h-abcdefgh".to_string(),
                },
            );
            write_envelope(
                stream,
                &HostOutbound::Data {
                    route: ROUTE.to_string(),
                    payload: json!({"e2e": "m", "v": 1, "ct": "AAAA"}),
                },
            );
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        let mut dropped = None;
        for _ in 0..10 {
            match next(&events) {
                Seen::Data(..) => panic!("data for an unknown route must not be delivered"),
                Seen::Rejected(code, message) => {
                    dropped = Some((code, message));
                    break;
                }
                _ => {}
            }
        }
        assert!(dropped.is_some(), "the drop must be reported");

        tunnel.stop();
        relay.join().ok();
    }

    #[test]
    fn an_oversized_payload_is_refused_before_it_is_delivered() {
        let (port, relay) = fake_relay(|stream| {
            assert_hello(&read_text(stream));
            write_envelope(
                stream,
                &HostOutbound::Ready {
                    protocol: envelope::RELAY_PROTOCOL,
                    host_id: "h-abcdefgh".to_string(),
                },
            );
            write_envelope(
                stream,
                &HostOutbound::Open {
                    route: ROUTE.to_string(),
                    peer: envelope::Peer {
                        address: "203.0.113.9".to_string(),
                    },
                    user_agent: None,
                },
            );
            write_envelope(
                stream,
                &HostOutbound::Data {
                    route: ROUTE.to_string(),
                    payload: json!({"e2e": "m", "ct": "A".repeat(envelope::MAX_PAYLOAD_BYTES)}),
                },
            );
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        let mut refused = false;
        for _ in 0..10 {
            match next(&events) {
                Seen::Data(..) => panic!("an oversized payload must never be delivered"),
                Seen::Rejected(code, _) if code == envelope::codes::TOO_LARGE => {
                    refused = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(refused, "an oversized payload must be refused");

        tunnel.stop();
        relay.join().ok();
    }

    #[test]
    fn stopping_releases_the_socket_and_does_not_retry() {
        let (port, relay) = fake_relay(|stream| {
            assert_hello(&read_text(stream));
            // Hold the connection open until the host stops.
            let _ = stream.shutdown(std::net::Shutdown::Read);
            std::thread::sleep(Duration::from_millis(50));
        });

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        assert_eq!(next(&events), Seen::Connecting);
        assert_eq!(next(&events), Seen::Online);

        tunnel.stop();
        assert_eq!(tunnel.status().state, TunnelState::Stopped);
        relay.join().ok();
    }

    #[test]
    fn an_unreachable_relay_reports_offline_and_keeps_retrying() {
        // Bind and immediately drop, so the port is almost certainly free.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };

        let (sink, events) = recorder();
        let tunnel = Tunnel::start(config(port), sink);

        assert_eq!(next(&events), Seen::Connecting);
        match next(&events) {
            Seen::Offline(reason) => assert!(!reason.is_empty()),
            other => panic!("expected offline, got {other:?}"),
        }

        tunnel.stop();
    }
}
