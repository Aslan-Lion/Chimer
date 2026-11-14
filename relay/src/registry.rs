//! The relay's routing core.
//!
//! The relay knows exactly three things:
//!
//! * which host ids currently have a live socket, and the queue that writes to
//!   each of them;
//! * which route ids are attached to which host, and the queue that writes to
//!   each device;
//! * the configured caps on hosts and on devices per host.
//!
//! It never parses a payload. Every function here moves opaque
//! [`serde_json::Value`]s between queues, which is what makes "the relay cannot
//! start a timer and cannot mint a credential" a structural property of the
//! code rather than a promise in a document.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;

use base64::Engine;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::wire::{self, DeviceOutbound, HostOutbound, Peer};

/// Frames that may be queued for one connection before the relay decides the
/// peer is too slow and gives up on it. A stuck reader must not be able to grow
/// the relay's memory without bound.
pub const OUTBOX: usize = 64;

/// Why a host socket was refused registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRefused {
    /// Another live socket already holds this host id.
    Conflict,
    /// The relay is already tracking `max_hosts` hosts.
    Full,
}

/// Why a device route could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRefused {
    /// No live socket exists for that host id.
    HostOffline,
    /// The host already has `max_devices_per_host` devices attached.
    Limit,
    /// The host's outbound queue is saturated.
    Slow,
    /// The relay could not obtain entropy for a route id.
    Internal,
}

/// Why a payload could not be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardRefused {
    /// The route id is unknown: never issued, or already closed.
    UnknownRoute,
    /// The destination's outbound queue is full.
    Slow,
    /// The other end of the route has gone away.
    Gone,
}

/// Counts for `/healthz`. Deliberately contains no identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub hosts: usize,
    pub devices: usize,
}

struct HostRegistration {
    sender: mpsc::Sender<HostOutbound>,
    /// The routes this host was told about. Bounded by `max_devices_per_host`.
    routes: HashSet<String>,
}

struct RouteRegistration {
    host_id: String,
    sender: mpsc::Sender<DeviceOutbound>,
}

#[derive(Default)]
struct Inner {
    hosts: HashMap<String, HostRegistration>,
    routes: HashMap<String, RouteRegistration>,
}

/// The host table and the route table, behind one lock.
///
/// A plain [`Mutex`] is used on purpose: every operation here is short and
/// non-blocking (the only concurrency primitive touched is `try_send`), and no
/// guard is ever held across an `await`. That keeps the whole routing decision
/// atomic without pulling in an async lock.
pub struct Registry {
    inner: Mutex<Inner>,
    max_hosts: usize,
    max_devices_per_host: usize,
    started: Instant,
}

impl Registry {
    pub fn new(max_hosts: usize, max_devices_per_host: usize) -> Registry {
        Registry {
            inner: Mutex::new(Inner::default()),
            max_hosts,
            max_devices_per_host,
            started: Instant::now(),
        }
    }

    /// Whole seconds since the relay started, for `/healthz`.
    pub fn uptime_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Registers a host socket. One host id has at most one live socket.
    pub fn register_host(
        &self,
        host_id: &str,
        sender: mpsc::Sender<HostOutbound>,
    ) -> Result<(), HostRefused> {
        let mut inner = self.inner.lock().unwrap();
        if inner.hosts.contains_key(host_id) {
            return Err(HostRefused::Conflict);
        }
        if inner.hosts.len() >= self.max_hosts {
            return Err(HostRefused::Full);
        }
        inner.hosts.insert(
            host_id.to_string(),
            HostRegistration {
                sender,
                routes: HashSet::new(),
            },
        );
        Ok(())
    }

    /// Removes a host socket and closes every device attached to it.
    ///
    /// Returns the route ids that were dropped, so the caller can log a count
    /// without ever touching what those routes were carrying.
    pub fn unregister_host(&self, host_id: &str) -> Vec<String> {
        let mut inner = self.inner.lock().unwrap();
        let Some(host) = inner.hosts.remove(host_id) else {
            return Vec::new();
        };

        let mut closed = Vec::with_capacity(host.routes.len());
        for route in host.routes {
            if let Some(registration) = inner.routes.remove(&route) {
                let _ = registration
                    .sender
                    .try_send(DeviceOutbound::Shutdown {
                        reason: "host-offline".to_string(),
                    });
            }
            closed.push(route);
        }
        closed
    }

    /// True when a host id currently has a live socket.
    ///
    /// Used by the route-isolation tests; the running relay learns the same
    /// fact from [`OpenRefused::HostOffline`] instead.
    #[cfg(test)]
    pub fn host_online(&self, host_id: &str) -> bool {
        self.inner.lock().unwrap().hosts.contains_key(host_id)
    }

    /// Allocates a fresh route for a device attached to `host_id`.
    ///
    /// The host is told a device arrived *before* the route is recorded, so a
    /// device can never send data the host was not already expecting.
    pub fn open_route(
        &self,
        host_id: &str,
        sender: mpsc::Sender<DeviceOutbound>,
        peer: Peer,
        user_agent: Option<String>,
    ) -> Result<String, OpenRefused> {
        let mut inner = self.inner.lock().unwrap();

        let host_sender = {
            let Some(host) = inner.hosts.get(host_id) else {
                return Err(OpenRefused::HostOffline);
            };
            if host.routes.len() >= self.max_devices_per_host {
                return Err(OpenRefused::Limit);
            }
            host.sender.clone()
        };

        let route = allocate_route(&inner).ok_or(OpenRefused::Internal)?;

        host_sender
            .try_send(HostOutbound::Open {
                route: route.clone(),
                peer,
                user_agent,
            })
            .map_err(|_| OpenRefused::Slow)?;

        inner.routes.insert(
            route.clone(),
            RouteRegistration {
                host_id: host_id.to_string(),
                sender,
            },
        );
        if let Some(host) = inner.hosts.get_mut(host_id) {
            host.routes.insert(route.clone());
        }
        Ok(route)
    }

    /// Removes a route.
    ///
    /// When `notify_host` is set the host is told the device went away; when the
    /// host itself asked for the close it already knows, and echoing would only
    /// produce noise. Returns the host id the route belonged to, if any.
    pub fn close_route(&self, route: &str, reason: &str, notify_host: bool) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        let registration = inner.routes.remove(route)?;
        let host_id = registration.host_id;

        if let Some(host) = inner.hosts.get_mut(&host_id) {
            host.routes.remove(route);
            if notify_host {
                let _ = host.sender.try_send(HostOutbound::Closed {
                    route: route.to_string(),
                    reason: reason.to_string(),
                });
            }
        }
        Some(host_id)
    }

    /// Moves one application payload from a device to its host.
    pub fn forward_to_host(&self, route: &str, payload: Value) -> Result<(), ForwardRefused> {
        let inner = self.inner.lock().unwrap();
        let Some(registration) = inner.routes.get(route) else {
            return Err(ForwardRefused::UnknownRoute);
        };
        let Some(host) = inner.hosts.get(&registration.host_id) else {
            return Err(ForwardRefused::Gone);
        };
        host.sender
            .try_send(HostOutbound::Data {
                route: route.to_string(),
                payload,
            })
            .map_err(classify)
    }

    /// Moves one application payload from a host to one of its devices.
    pub fn forward_to_device(&self, route: &str, payload: Value) -> Result<(), ForwardRefused> {
        let inner = self.inner.lock().unwrap();
        let Some(registration) = inner.routes.get(route) else {
            return Err(ForwardRefused::UnknownRoute);
        };
        registration
            .sender
            .try_send(DeviceOutbound::Data { payload })
            .map_err(classify)
    }

    /// How many devices are currently attached to a host.
    ///
    /// Used by the tests that check a host's device budget is released again
    /// when a device goes away.
    #[cfg(test)]
    pub fn devices_for_host(&self, host_id: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .hosts
            .get(host_id)
            .map(|host| host.routes.len())
            .unwrap_or_default()
    }

    pub fn stats(&self) -> Stats {
        let inner = self.inner.lock().unwrap();
        Stats {
            hosts: inner.hosts.len(),
            devices: inner.routes.len(),
        }
    }

    /// Asks everyone to go away, without waiting for replies.
    ///
    /// Used on the graceful shutdown path so a phone sees "the relay is
    /// restarting" rather than a bare socket error. Nothing is persisted here;
    /// the maps are simply dropped.
    pub fn notify_everyone_of_shutdown(&self, reason: &str) {
        let mut inner = self.inner.lock().unwrap();
        for (_, registration) in inner.routes.drain() {
            let _ = registration.sender.try_send(DeviceOutbound::Shutdown {
                reason: reason.to_string(),
            });
        }
        for (_, host) in inner.hosts.drain() {
            let _ = host.sender.try_send(HostOutbound::Error {
                code: wire::codes::SHUTTING_DOWN.to_string(),
                message: reason.to_string(),
            });
        }
    }
}

fn classify<T>(error: mpsc::error::TrySendError<T>) -> ForwardRefused {
    match error {
        mpsc::error::TrySendError::Full(_) => ForwardRefused::Slow,
        mpsc::error::TrySendError::Closed(_) => ForwardRefused::Gone,
    }
}

/// Draws route ids until one is not already in use.
///
/// The chance of a collision is negligible (256 bits), so the retry loop exists
/// only to make the invariant explicit rather than to be exercised.
fn allocate_route(inner: &Inner) -> Option<String> {
    for _ in 0..8 {
        let candidate = new_route_id().ok()?;
        if !inner.routes.contains_key(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// A fresh 256-bit route id, base64url without padding.
fn new_route_id() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; wire::ROUTE_ID_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registry() -> Registry {
        Registry::new(4, 2)
    }

    fn peer() -> Peer {
        Peer {
            address: "203.0.113.9:41234".to_string(),
        }
    }

    #[test]
    fn a_host_registers_once_and_a_second_socket_is_refused() {
        let registry = registry();
        let (tx, _rx) = mpsc::channel(OUTBOX);
        assert_eq!(registry.register_host("h-abcdefgh", tx.clone()), Ok(()));
        assert!(registry.host_online("h-abcdefgh"));

        assert_eq!(
            registry.register_host("h-abcdefgh", tx),
            Err(HostRefused::Conflict)
        );
    }

    #[test]
    fn the_host_cap_is_enforced() {
        let registry = Registry::new(1, 2);
        let (tx, _rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-aaaaaaaa", tx.clone()).unwrap();
        assert_eq!(
            registry.register_host("h-bbbbbbbb", tx),
            Err(HostRefused::Full)
        );
    }

    #[test]
    fn a_route_id_is_a_256_bit_base64url_word() {
        let route = new_route_id().unwrap();
        assert_eq!(route.len(), wire::ROUTE_ID_LEN);
        assert!(wire::valid_route_id(&route));
        assert_ne!(route, new_route_id().unwrap());
    }

    #[test]
    fn opening_a_route_tells_the_host_then_hands_the_id_back() {
        let registry = registry();
        let (host_tx, mut host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();

        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        let route = registry
            .open_route("h-abcdefgh", device_tx, peer(), Some("phone".to_string()))
            .unwrap();

        match host_rx.try_recv().unwrap() {
            HostOutbound::Open {
                route: announced,
                user_agent,
                ..
            } => {
                assert_eq!(announced, route);
                assert_eq!(user_agent.as_deref(), Some("phone"));
            }
            other => panic!("expected an open, got {other:?}"),
        }
        assert_eq!(registry.devices_for_host("h-abcdefgh"), 1);
    }

    #[test]
    fn a_device_cannot_attach_to_a_host_that_is_not_online() {
        let registry = registry();
        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        assert_eq!(
            registry.open_route("h-abcdefgh", device_tx, peer(), None),
            Err(OpenRefused::HostOffline)
        );
    }

    #[test]
    fn the_per_host_device_cap_is_enforced() {
        let registry = registry(); // two devices per host
        let (host_tx, _host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();

        for _ in 0..2 {
            let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
            registry
                .open_route("h-abcdefgh", device_tx, peer(), None)
                .unwrap();
        }

        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        assert_eq!(
            registry.open_route("h-abcdefgh", device_tx, peer(), None),
            Err(OpenRefused::Limit)
        );
    }

    #[test]
    fn a_saturated_host_queue_refuses_the_device_instead_of_blocking() {
        let registry = Registry::new(4, 8);
        // A one-frame queue that nobody drains: the second open must be refused.
        let (host_tx, _host_rx) = mpsc::channel(1);
        registry.register_host("h-abcdefgh", host_tx).unwrap();

        let (first_tx, _first_rx) = mpsc::channel(OUTBOX);
        registry
            .open_route("h-abcdefgh", first_tx, peer(), None)
            .unwrap();

        let (second_tx, _second_rx) = mpsc::channel(OUTBOX);
        assert_eq!(
            registry.open_route("h-abcdefgh", second_tx, peer(), None),
            Err(OpenRefused::Slow)
        );
        // The refused attempt left nothing behind.
        assert_eq!(registry.devices_for_host("h-abcdefgh"), 1);
    }

    #[test]
    fn a_device_payload_reaches_its_host_tagged_with_the_route() {
        let registry = registry();
        let (host_tx, mut host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();
        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        let route = registry
            .open_route("h-abcdefgh", device_tx, peer(), None)
            .unwrap();
        let _ = host_rx.try_recv().unwrap(); // the open

        registry
            .forward_to_host(&route, json!({"type":"pair"}))
            .unwrap();

        match host_rx.try_recv().unwrap() {
            HostOutbound::Data {
                route: carried,
                payload,
            } => {
                assert_eq!(carried, route);
                assert_eq!(payload["type"], json!("pair"));
            }
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn a_host_payload_reaches_the_device_unnamed() {
        let registry = registry();
        let (host_tx, _host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();
        let (device_tx, mut device_rx) = mpsc::channel(OUTBOX);
        let route = registry
            .open_route("h-abcdefgh", device_tx, peer(), None)
            .unwrap();

        registry
            .forward_to_device(&route, json!({"type":"state"}))
            .unwrap();

        match device_rx.try_recv().unwrap() {
            DeviceOutbound::Data { payload } => assert_eq!(payload["type"], json!("state")),
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn traffic_for_an_unknown_route_is_refused_rather_than_broadcast() {
        let registry = registry();
        assert_eq!(
            registry.forward_to_host("unknown", json!({"type":"command"})),
            Err(ForwardRefused::UnknownRoute)
        );
        assert_eq!(
            registry.forward_to_device("unknown", json!({"type":"state"})),
            Err(ForwardRefused::UnknownRoute)
        );
    }

    #[test]
    fn closing_a_route_notifies_the_host_only_when_asked() {
        let registry = registry();
        let (host_tx, mut host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();

        let (first_tx, _first_rx) = mpsc::channel(OUTBOX);
        let first = registry
            .open_route("h-abcdefgh", first_tx, peer(), None)
            .unwrap();
        let _ = host_rx.try_recv().unwrap();

        let (second_tx, _second_rx) = mpsc::channel(OUTBOX);
        let second = registry
            .open_route("h-abcdefgh", second_tx, peer(), None)
            .unwrap();
        let _ = host_rx.try_recv().unwrap();

        // The host asked for this one, so it is not told what it already knows.
        assert_eq!(registry.close_route(&first, "revoked", false).as_deref(), Some("h-abcdefgh"));
        assert!(host_rx.try_recv().is_err(), "the host already asked for this close");
        assert_eq!(registry.stats().devices, 1, "only the closed route is gone");
        assert_eq!(registry.forward_to_host(&first, json!({})), Err(ForwardRefused::UnknownRoute));

        // A device that simply vanished does get announced.
        assert_eq!(registry.close_route(&second, "peer-gone", true).as_deref(), Some("h-abcdefgh"));
        match host_rx.try_recv().unwrap() {
            HostOutbound::Closed { route, reason } => {
                assert_eq!(route, second);
                assert_eq!(reason, "peer-gone");
            }
            other => panic!("expected a closed, got {other:?}"),
        }
    }

    #[test]
    fn losing_a_host_closes_every_route_it_owned() {
        let registry = registry();
        let (host_tx, _host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();

        let (first_tx, mut first_rx) = mpsc::channel(OUTBOX);
        let first = registry
            .open_route("h-abcdefgh", first_tx, peer(), None)
            .unwrap();
        let (second_tx, mut second_rx) = mpsc::channel(OUTBOX);
        let _second = registry
            .open_route("h-abcdefgh", second_tx, peer(), None)
            .unwrap();

        let mut closed = registry.unregister_host("h-abcdefgh");
        closed.sort();
        assert_eq!(closed.len(), 2);
        assert!(closed.contains(&first));

        // Both devices are told to stop, and the routes are gone.
        for receiver in [&mut first_rx, &mut second_rx] {
            assert!(matches!(
                receiver.try_recv().unwrap(),
                DeviceOutbound::Shutdown { .. }
            ));
        }
        assert_eq!(registry.stats().devices, 0);
        assert_eq!(
            registry.forward_to_host(&first, json!({})),
            Err(ForwardRefused::UnknownRoute)
        );
    }

    #[test]
    fn stats_count_hosts_and_devices_and_never_identify_them() {
        let registry = registry();
        let (host_tx, _host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();
        let (device_tx, _device_rx) = mpsc::channel(OUTBOX);
        registry
            .open_route("h-abcdefgh", device_tx, peer(), None)
            .unwrap();

        let stats = registry.stats();
        assert_eq!(stats.hosts, 1);
        assert_eq!(stats.devices, 1);
    }

    #[test]
    fn shutdown_asks_everyone_to_stop_without_persisting_anything() {
        let registry = registry();
        let (host_tx, mut host_rx) = mpsc::channel(OUTBOX);
        registry.register_host("h-abcdefgh", host_tx).unwrap();
        let (device_tx, mut device_rx) = mpsc::channel(OUTBOX);
        registry
            .open_route("h-abcdefgh", device_tx, peer(), None)
            .unwrap();
        let _ = host_rx.try_recv().unwrap(); // the open

        registry.notify_everyone_of_shutdown("the relay is restarting");

        assert!(matches!(
            host_rx.try_recv().unwrap(),
            HostOutbound::Error { code, .. } if code == wire::codes::SHUTTING_DOWN
        ));
        assert!(matches!(
            device_rx.try_recv().unwrap(),
            DeviceOutbound::Shutdown { .. }
        ));
        assert_eq!(registry.stats(), Stats { hosts: 0, devices: 0 });
    }
}
