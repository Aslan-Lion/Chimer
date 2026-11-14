//! Local network IPC commands.
//!
//! Thin adapters over [`NetworkEngine`], mirroring [`crate::timer::commands`]
//! and [`crate::session::commands`]: they translate the argument shapes and
//! hand the engine the application handle it needs to broadcast.
//!
//! None of these commands creates or modifies a native window, so they run
//! synchronously on the main thread. Binding the listener and spawning the
//! accept loop are both quick and non-blocking, so nothing here waits on a
//! socket: a device connecting is handled on its own thread inside the engine.

use std::sync::Arc;

use tauri::{AppHandle, State};

use super::engine::{DeviceView, NetworkEngine};
use super::model::{
    ConnectedDevice, DevicePermission, DeviceRole, NetworkAddress, NetworkError, NetworkStatus,
    PairingRequest,
};
use super::protocol::{LibraryMirror, RemotePreset, RemoteSession};
use super::qr::PairingCode;

type Engine<'a> = State<'a, Arc<NetworkEngine>>;

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

/// The status the Settings page renders.
#[tauri::command]
pub fn network_status(engine: Engine<'_>) -> NetworkStatus {
    engine.status()
}

/// Re-reads the machine's network interfaces and returns the fresh status.
#[tauri::command]
pub fn network_refresh(engine: Engine<'_>, app: AppHandle) -> NetworkStatus {
    engine.refresh_addresses(&app)
}

/// Starts the local server on `port`, bound to `address` (or the best local
/// address when omitted).
#[tauri::command]
pub fn network_start(
    engine: Engine<'_>,
    app: AppHandle,
    port: u16,
    address: Option<String>,
    bind_all: bool,
) -> Result<NetworkStatus, NetworkError> {
    engine.start(&app, port, address, bind_all)
}

/// Stops the local server and drops every connection.
#[tauri::command]
pub fn network_stop(engine: Engine<'_>, app: AppHandle) -> NetworkStatus {
    engine.stop(&app);
    engine.status()
}

/// The interfaces a device could reach this host on.
#[tauri::command]
pub fn network_addresses(engine: Engine<'_>) -> Vec<NetworkAddress> {
    engine.status().addresses
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

/// Issues a fresh, short-lived pairing code and the QR code for it.
#[tauri::command]
pub fn network_issue_pairing(engine: Engine<'_>, app: AppHandle) -> Result<PairingCode, String> {
    engine.issue_pairing(&app)
}

/// Pairing requests currently waiting for the operator's decision.
#[tauri::command]
pub fn network_pending_requests(engine: Engine<'_>) -> Vec<PairingRequest> {
    engine.pending_requests()
}

/// Approves a request and gives the device its role.
#[tauri::command]
pub fn network_approve_pairing(
    engine: Engine<'_>,
    app: AppHandle,
    request_id: String,
    role: DeviceRole,
) -> Result<(), String> {
    engine.approve(&app, &request_id, role)
}

/// Declines a request. The pairing code stays usable.
#[tauri::command]
pub fn network_reject_pairing(
    engine: Engine<'_>,
    app: AppHandle,
    request_id: String,
) -> Result<(), String> {
    engine.reject(&app, &request_id)
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/// Every paired device, with its live connection state.
#[tauri::command]
pub fn network_devices(engine: Engine<'_>) -> Vec<DeviceView> {
    engine.device_views()
}

/// Every live connection, including ones that have not paired yet.
#[tauri::command]
pub fn network_connections(engine: Engine<'_>) -> Vec<ConnectedDevice> {
    engine.connected_devices()
}

/// Changes a device's role.
#[tauri::command]
pub fn network_set_role(
    engine: Engine<'_>,
    app: AppHandle,
    device_id: String,
    role: DeviceRole,
) -> Result<(), String> {
    engine.set_role(&app, &device_id, role)
}

/// Replaces a device's individual permissions.
#[tauri::command]
pub fn network_set_permissions(
    engine: Engine<'_>,
    app: AppHandle,
    device_id: String,
    permissions: Vec<DevicePermission>,
) -> Result<(), String> {
    engine.set_permissions(&app, &device_id, permissions)
}

/// Revokes a device: its token stops working and its sockets are closed.
#[tauri::command]
pub fn network_revoke_device(
    engine: Engine<'_>,
    app: AppHandle,
    device_id: String,
) -> Result<(), String> {
    engine.revoke(&app, &device_id)
}

/// Drops one connection immediately, without revoking the device.
#[tauri::command]
pub fn network_disconnect(
    engine: Engine<'_>,
    app: AppHandle,
    connection_id: String,
) -> Result<(), String> {
    engine.disconnect(&app, &connection_id)
}

// ---------------------------------------------------------------------------
// Library mirror
// ---------------------------------------------------------------------------

/// Publishes the operator's presets and sessions to the paired controllers.
///
/// The host resolves a remote's `preset.start` / `session.start` against this
/// same mirror, so a remote can never start anything the host does not have.
/// The mirror is only mirrored when it actually changed.
#[tauri::command]
pub fn network_set_library(
    engine: Engine<'_>,
    app: AppHandle,
    presets: Vec<RemotePreset>,
    sessions: Vec<RemoteSession>,
) {
    engine.set_library(
        &app,
        LibraryMirror {
            presets,
            sessions,
        },
    );
}
