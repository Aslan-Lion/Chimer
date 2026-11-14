//! Broadcast helpers for the local network engine.
//!
//! The Settings page is the only consumer: it wants to know when the server
//! starts or stops, when a device is paired, revoked or reconnected, and when
//! a device is knocking for approval. The channel names themselves live in
//! [`crate::events`].

use tauri::{AppHandle, Emitter};

use crate::events as channels;

use super::engine::DeviceView;
use super::model::{NetworkStatus, PairingRequest};

/// Broadcasts the server's status over `network://status-changed`.
pub fn emit_status(app: &AppHandle, status: &NetworkStatus) {
    let _ = app.emit(channels::NETWORK_STATUS_CHANGED, status);
}

/// Broadcasts the paired-device list over `network://devices-changed`.
pub fn emit_devices(app: &AppHandle, devices: &[DeviceView]) {
    let _ = app.emit(channels::NETWORK_DEVICES_CHANGED, devices);
}

/// Broadcasts the pending pairing requests over `network://pairing-changed`.
pub fn emit_pairing(app: &AppHandle, requests: &[PairingRequest]) {
    let _ = app.emit(channels::NETWORK_PAIRING_CHANGED, requests);
}
