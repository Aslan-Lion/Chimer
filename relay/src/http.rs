//! The relay's HTTP surface.
//!
//! Three things are served:
//!
//! * `GET /healthz` — a health check with counts and nothing identifying;
//! * `GET /ws/host` — the leg the Windows host dials *out* to, so the desktop
//!   application never needs an inbound port, a public address or a router rule;
//! * `GET /ws/device/{host}` — the leg a phone, tablet or browser dials.
//!
//! Everything else falls through to the built remote application, so a
//! deployment serves the controller and the relay from one origin and the
//! browser's WebSocket connections are same-origin by construction.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::limits::Limits;
use crate::logging;
use crate::registry::Registry;
use crate::session;
use crate::wire;

/// Everything the handlers need, cloned per request.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub registry: Arc<Registry>,
    pub limits: Arc<Mutex<Limits>>,
}

/// The whole relay, as one router.
pub fn router(state: AppState) -> Router {
    let files = ServeDir::new(state.config.web_root.clone());

    Router::<AppState>::new()
        .route("/healthz", get(health))
        .route("/ws/host", get(host_upgrade))
        .route("/ws/device/{host_id}", get(device_upgrade))
        .fallback_service(files)
        .with_state(state)
}

/// What `/healthz` reports. No host identifiers, no device identifiers, no
/// addresses: a health check must not become an inventory.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    status: &'static str,
    protocol: u32,
    hosts: usize,
    devices: usize,
    uptime_seconds: u64,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    let stats = state.registry.stats();
    Json(Health {
        status: "ok",
        protocol: wire::RELAY_PROTOCOL,
        hosts: stats.hosts,
        devices: stats.devices,
        uptime_seconds: state.registry.uptime_seconds(),
    })
}

/// The host leg. Authenticated by the credential in the opening `hello`.
async fn host_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !allow_connection(&state, peer.ip()) {
        logging::warn("host-connection-limited", &[("peer", peer.ip().to_string())]);
        return too_many_requests();
    }

    let address = peer.ip().to_string();
    let user_agent = header_text(&headers, header::USER_AGENT);
    let limit = state.config.max_frame_bytes;

    ws.max_message_size(limit)
        .max_frame_size(limit)
        .on_upgrade(move |socket| session::host_session(socket, state, address, user_agent))
}

/// The device leg. The host identifier is in the path because a device has to
/// say *which* host it wants before the relay can route anything, but knowing
/// the identifier grants nothing: the host still has to pair the device.
async fn device_upgrade(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !wire::valid_host_id(&host_id) {
        // Rejected before the handshake: a malformed identifier costs the relay
        // nothing to refuse, and the answer is identical for every host.
        return not_found();
    }
    if !origin_allowed(&state.config, &headers) {
        logging::warn(
            "device-origin-refused",
            &[("peer", peer.ip().to_string())],
        );
        return forbidden();
    }
    if !allow_connection(&state, peer.ip()) {
        logging::warn("device-connection-limited", &[("peer", peer.ip().to_string())]);
        return too_many_requests();
    }

    let address = peer.ip().to_string();
    let user_agent = header_text(&headers, header::USER_AGENT);
    let limit = state.config.max_frame_bytes;

    ws.max_message_size(limit)
        .max_frame_size(limit)
        .on_upgrade(move |socket| session::device_session(socket, state, host_id, address, user_agent))
}

/// Whether an `Origin` header, if present, is one this relay serves.
///
/// A browser always sends `Origin` on a WebSocket handshake, so this is the
/// standard cross-site-connection defence. A non-browser client sends none, and
/// is allowed through: it must still pair with the host and authenticate with
/// its own device token before it can do anything, so an absent `Origin` is not
/// authority.
fn origin_allowed(config: &Config, headers: &HeaderMap) -> bool {
    let Some(origin) = header_text(headers, header::ORIGIN) else {
        return true;
    };

    if config.allowed_origins.iter().any(|allowed| allowed == &origin) {
        return true;
    }

    match &config.public_origin {
        Some(expected) => expected == &origin,
        None => same_origin_as_request(&origin, config, headers),
    }
}

/// Compares an `Origin` with the `Host` the request actually arrived on.
fn same_origin_as_request(origin: &str, config: &Config, headers: &HeaderMap) -> bool {
    let Some(host) = header_text(headers, header::HOST) else {
        return false;
    };
    let scheme = if config.tls_enabled() { "https" } else { "http" };
    origin == format!("{scheme}://{host}")
}

/// Records one WebSocket handshake against the source address's budget.
fn allow_connection(state: &AppState, ip: IpAddr) -> bool {
    state
        .limits
        .lock()
        .unwrap()
        .connections
        .check(&ip.to_string(), logging::now_ms())
}

fn header_text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found.").into_response()
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"))],
        "This origin may not open a relay connection.",
    )
        .into_response()
}

fn too_many_requests() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"))],
        "Too many connections from this address; slow down.",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Config {
        let mut vars: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        vars.insert("RELAY_TLS_CERT".to_string(), "/c.pem".to_string());
        vars.insert("RELAY_TLS_KEY".to_string(), "/k.pem".to_string());
        Config::parse(vars).unwrap()
    }

    /// The development shape: no TLS material, insecure mode, loopback bind.
    fn plaintext_config() -> Config {
        Config::parse(BTreeMap::from([(
            "RELAY_ALLOW_INSECURE".to_string(),
            "1".to_string(),
        )]))
        .unwrap()
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn a_request_without_an_origin_is_allowed_through_to_pairing() {
        let config = config(&[]);
        assert!(origin_allowed(&config, &headers(&[])));
    }

    #[test]
    fn an_origin_matching_the_request_host_is_accepted() {
        let config = config(&[]);
        let headers = headers(&[("origin", "https://relay.example"), ("host", "relay.example")]);
        assert!(origin_allowed(&config, &headers));
    }

    #[test]
    fn an_origin_from_another_site_is_refused() {
        let config = config(&[]);
        let headers = headers(&[
            ("origin", "https://attacker.example"),
            ("host", "relay.example"),
        ]);
        assert!(!origin_allowed(&config, &headers));
    }

    #[test]
    fn a_configured_public_origin_is_honoured_exactly() {
        let config = config(&[("RELAY_PUBLIC_ORIGIN", "https://timer.example")]);

        assert!(origin_allowed(
            &config,
            &headers(&[("origin", "https://timer.example"), ("host", "10.0.0.5:8443")])
        ));
        // Same host, different scheme: still refused, because the origin was
        // configured explicitly and must match character for character.
        assert!(!origin_allowed(
            &config,
            &headers(&[("origin", "http://timer.example"), ("host", "timer.example")])
        ));
    }

    #[test]
    fn extra_origins_can_be_allow_listed() {
        let config = config(&[("RELAY_ALLOWED_ORIGINS", "https://a.example,https://b.example")]);
        assert!(origin_allowed(
            &config,
            &headers(&[("origin", "https://b.example"), ("host", "relay.example")])
        ));
        assert!(!origin_allowed(
            &config,
            &headers(&[("origin", "https://c.example"), ("host", "relay.example")])
        ));
    }

    #[test]
    fn the_scheme_is_derived_from_the_tls_configuration() {
        // With TLS the matching origin is https, so an http origin is another
        // scheme on the same host and must be refused.
        let secure = config(&[]);
        assert!(!origin_allowed(
            &secure,
            &headers(&[("origin", "http://relay.example"), ("host", "relay.example")])
        ));
        assert!(origin_allowed(
            &secure,
            &headers(&[("origin", "https://relay.example"), ("host", "relay.example")])
        ));

        // In the loopback development shape the scheme is http, and the check
        // flips with it.
        let plaintext = plaintext_config();
        assert!(origin_allowed(
            &plaintext,
            &headers(&[("origin", "http://127.0.0.1:8788"), ("host", "127.0.0.1:8788")])
        ));
        assert!(!origin_allowed(
            &plaintext,
            &headers(&[("origin", "https://127.0.0.1:8788"), ("host", "127.0.0.1:8788")])
        ));
    }

    #[test]
    fn a_missing_host_header_makes_the_same_origin_check_fail_closed() {
        let config = config(&[]);
        assert!(!origin_allowed(
            &config,
            &headers(&[("origin", "https://relay.example")])
        ));
    }
}
