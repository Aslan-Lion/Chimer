//! The app.timer online relay.
//!
//! A separate, deployable service that lets a paired phone, tablet or computer
//! reach a Windows app.timer host over the internet, across two different
//! networks, without the host opening a port, holding a public address, or
//! touching a router.
//!
//! The relay is a dumb pipe. It moves opaque JSON objects between a host socket
//! and device sockets selected by an unguessable route identifier, terminates
//! TLS on both legs, and keeps nothing on disk. It has no timer engine, no
//! session state and no idea what any forwarded message means — so it cannot
//! become a second source of truth even by accident.
//!
//! Run it with the environment described in `README.md`: a bad configuration
//! fails at start-up and names the file or variable that is wrong.
//!
//! # Trust boundary
//!
//! TLS ends at the relay on both legs, so the operator of this process can read
//! and alter anything a remote device sends to a host and anything a host sends
//! back. **This is not end-to-end encryption.** The relay is trusted for
//! confidentiality of the transport, and trusted not to lie about *identity* —
//! device tokens are hashed at rest and the host authenticates every device
//! itself — but it is deliberately not trusted for authority over timer state.

pub mod config;
pub mod http;
pub mod limits;
pub mod logging;
pub mod registry;
pub mod session;
pub mod tls;
pub mod wire;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum_server::tls_rustls::RustlsConfig;
use axum_server::Handle;

use crate::config::Config;
use crate::http::AppState;
use crate::limits::Limits;
use crate::registry::Registry;

/// Builds the shared state the router and both session state machines need.
///
/// Split out of [`serve`] so a test can inspect the registry it is about to
/// drive.
pub fn state(config: Config) -> AppState {
    let registry = Arc::new(Registry::new(
        config.max_hosts,
        config.max_devices_per_host,
    ));
    let limits = Arc::new(Mutex::new(Limits::new(
        config.connections_per_minute,
        config.data_frames_per_sec,
    )));

    AppState {
        config: Arc::new(config),
        registry,
        limits,
    }
}

/// Runs the listener until `handle` is asked to shut it down.
///
/// Shared by the binary and the end-to-end tests, so both drive the identical
/// stack: the same routes, the same TLS decision, the same connect-info
/// plumbing. `tls` must already be loaded; see [`tls::load`].
pub async fn serve(
    handle: Handle<SocketAddr>,
    state: AppState,
    tls: Option<RustlsConfig>,
) -> Result<(), String> {
    let bind = state.config.bind;
    let app = http::router(state);
    let make_service = app.into_make_service_with_connect_info::<SocketAddr>();

    let outcome = match tls {
        Some(tls) => {
            axum_server::bind_rustls(bind, tls)
                .handle(handle)
                .serve(make_service)
                .await
        }
        None => {
            axum_server::bind(bind)
                .handle(handle)
                .serve(make_service)
                .await
        }
    };

    outcome.map_err(|error| {
        format!(
            "the listener on {bind} could not run: {error}. If the port is already in use, stop \
             the other process or set RELAY_BIND to a free address."
        )
    })
}
