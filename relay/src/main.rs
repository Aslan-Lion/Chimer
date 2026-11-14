//! The app.timer online relay: process entry point.
//!
//! Everything of substance lives in the library so the end-to-end tests can
//! start the real listener; see `lib.rs` for the architecture and the trust
//! boundary.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum_server::Handle;

use app_timer_relay::config::Config;
use app_timer_relay::limits::Limits;
use app_timer_relay::registry::Registry;
use app_timer_relay::{logging, serve, state, tls, wire};

/// How often rate-limiter history is aged out.
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60);

/// How long a graceful shutdown waits for connections to end by themselves.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        logging::error("startup-failed", &[("reason", error)]);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env()?;
    logging::init(config.log_level);

    // Loaded before anything is logged, so a bad certificate is reported
    // against a clean start-up banner rather than after "starting".
    let tls = match &config.tls {
        Some(material) => Some(tls::load(material).await?),
        None => None,
    };

    logging::info(
        "starting",
        &[
            ("bind", config.bind.to_string()),
            (
                "tls",
                if config.tls_enabled() {
                    "enabled".to_string()
                } else {
                    "disabled (loopback development only)".to_string()
                },
            ),
            ("hosts", config.hosts.len().to_string()),
            ("webRoot", config.web_root.display().to_string()),
            ("protocol", wire::RELAY_PROTOCOL.to_string()),
        ],
    );

    if !config.web_root.join("index.html").is_file() {
        logging::warn(
            "web-root-missing",
            &[
                ("webRoot", config.web_root.display().to_string()),
                (
                    "effect",
                    "the relay will route traffic but serve no remote application".to_string(),
                ),
            ],
        );
    }

    let state = state(config);
    // The maintenance task must prune the *same* limiter, and the shutdown
    // notice must reach the *same* registry, that the router is serving.
    let registry = state.registry.clone();
    let limits = state.limits.clone();

    let handle = Handle::new();
    tokio::spawn(shutdown_signal(handle.clone(), registry, limits));

    serve(handle, state, tls).await?;
    logging::info("stopped", &[]);
    Ok(())
}

/// Waits for Ctrl-C (any platform) or SIGTERM (unix), pruning rate-limiter
/// history while it waits.
async fn shutdown_signal(
    handle: Handle<SocketAddr>,
    registry: Arc<Registry>,
    limits: Arc<Mutex<Limits>>,
) {
    let mut maintenance = tokio::time::interval(MAINTENANCE_INTERVAL);
    maintenance.tick().await; // the first tick is immediate; nothing to prune yet

    let mut signal = Box::pin(wait_for_signal());
    loop {
        tokio::select! {
            _ = &mut signal => break,
            _ = maintenance.tick() => {
                limits.lock().unwrap().prune();
                let stats = registry.stats();
                logging::debug("maintenance", &[
                    ("hosts", stats.hosts.to_string()),
                    ("devices", stats.devices.to_string()),
                ]);
            }
        }
    }

    logging::info("shutdown-requested", &[]);
    // Everyone is told why, so a phone shows "the relay is restarting" instead
    // of an unexplained socket error. Nothing is written to disk.
    registry.notify_everyone_of_shutdown("The relay is shutting down.");
    handle.graceful_shutdown(Some(SHUTDOWN_GRACE));
}

/// Resolves when the process is asked to stop.
async fn wait_for_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
