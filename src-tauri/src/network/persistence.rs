//! Device persistence.
//!
//! The paired-device register has to survive a restart, exactly as the timer
//! and session documents do. [`crate::database`] is still only a stub reserved
//! for Milestone 8, so this follows the convention the other engines already
//! use: one small JSON document in the platform application-data directory,
//! written atomically through a temporary file and a rename.
//!
//! What is *not* written here is any secret. A device record stores the SHA-256
//! digest of its token, never the token; there is no plaintext credential in the
//! document, and there is nothing in it a reader could replay.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::model::PairedDevice;

/// File the paired-device register is written to, inside the app data directory.
pub const NETWORK_DEVICES_FILE: &str = "network-devices.json";

/// On-disk shape. Versioned so a future format change can be recognised rather
/// than misread.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDocument {
    pub version: u32,
    #[serde(default)]
    pub devices: Vec<PairedDevice>,
}

impl Default for NetworkDocument {
    fn default() -> Self {
        Self {
            version: 1,
            devices: Vec::new(),
        }
    }
}

/// Absolute path of the register, creating the app data directory if needed.
pub fn resolve_path(app: &AppHandle) -> Option<PathBuf> {
    let directory = app.path().app_data_dir().ok()?;
    if fs::create_dir_all(&directory).is_err() {
        return None;
    }
    Some(directory.join(NETWORK_DEVICES_FILE))
}

/// Reads the register.
///
/// A missing, unreadable or corrupted document is reported as "no devices"
/// rather than failing, so the application always starts and simply asks the
/// operator to pair again.
pub fn load(path: &Path) -> Vec<PairedDevice> {
    let Some(raw) = fs::read_to_string(path).ok() else {
        return Vec::new();
    };
    match serde_json::from_str::<NetworkDocument>(&raw) {
        Ok(document) => document.devices,
        Err(_) => Vec::new(),
    }
}

/// Writes the register atomically, replacing the previous document.
pub fn save(path: &Path, devices: &[PairedDevice]) {
    let document = NetworkDocument {
        version: 1,
        devices: devices.to_vec(),
    };
    let Ok(payload) = serde_json::to_string_pretty(&document) else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    if fs::write(&temporary, payload).is_err() {
        return;
    }
    let _ = fs::rename(&temporary, path);
}

/// Removes the document, so revoking every device leaves nothing behind.
pub fn clear(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::model::{DeviceKind, DevicePermission, DeviceRole};
    use crate::network::tokens::hash_token;

    fn device(id: &str) -> PairedDevice {
        PairedDevice {
            id: id.to_string(),
            name: "Pixel".to_string(),
            kind: DeviceKind::Phone,
            role: DeviceRole::Controller,
            permissions: vec![DevicePermission::TimerStart],
            token_hash: hash_token("super-secret-token"),
            paired_at: 1,
            last_seen_at: 2,
            user_agent: "test".to_string(),
            device_static_pk: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("app-timer-network-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the temporary directory is writable");
        path.join(NETWORK_DEVICES_FILE)
    }

    #[test]
    fn a_saved_register_round_trips() {
        let path = scratch("round-trip");
        save(&path, &[device("device-1")]);

        let loaded = load(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "device-1");
        assert_eq!(loaded[0].role, DeviceRole::Controller);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_document_never_contains_a_plaintext_token() {
        let path = scratch("no-plaintext");
        save(&path, &[device("device-1")]);

        let raw = fs::read_to_string(&path).expect("the document was just written");
        assert!(
            !raw.contains("super-secret-token"),
            "a device token must never be written to disk"
        );
        assert!(raw.contains(&hash_token("super-secret-token")));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_document_loads_as_no_devices() {
        let path = scratch("missing");
        assert!(load(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_corrupted_document_loads_as_no_devices() {
        let path = scratch("corrupted");
        fs::write(&path, "{ this is not json").expect("the scratch file is writable");
        assert!(load(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clearing_removes_the_document() {
        let path = scratch("clear");
        save(&path, &[device("device-1")]);
        clear(&path);
        assert!(load(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
