//! The online settings document.
//!
//! `online-settings.json` sits beside the M6 `network-devices.json` and holds
//! only what the host needs to reconnect and be recognised by its relay: the
//! relay URL, the host id the relay knows it by, the host's **public** identity
//! key, and — when the operator has not supplied it through the environment —
//! the relay credential. It is written with the same temp-file-plus-rename
//! discipline as the device register, and it is never logged.
//!
//! What is deliberately **not** here: the host's private identity key (sealed in
//! its own DPAPI-protected file, see [`super::identity`]), any device token, and
//! any pairing code. The device registry is not duplicated online — the M6
//! `network-devices.json` remains the single register, extended by one field.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// File the online settings are written to, inside the app data directory.
pub const ONLINE_SETTINGS_FILE: &str = "online-settings.json";

/// Environment variable that overrides the stored relay credential.
pub const RELAY_TOKEN_ENV: &str = "APP_TIMER_RELAY_TOKEN";

/// On-disk shape. Every field after `version` defaults, so a partially written
/// or older document still loads and the host can be reconfigured rather than
/// failing to start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSettingsDocument {
    pub version: u32,
    #[serde(default)]
    pub enabled: bool,
    /// Relay base URL, e.g. `wss://relay.example.com`.
    #[serde(default)]
    pub relay_url: String,
    /// Locally generated identifier the relay knows this host by. Not a secret.
    #[serde(default)]
    pub host_id: String,
    /// The host's public identity key `H_pub`, base64url without padding.
    #[serde(default)]
    pub host_public_key: String,
    /// Relay credential. `None` when it is supplied through the environment.
    #[serde(default)]
    pub host_token: Option<String>,
}

impl Default for OnlineSettingsDocument {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            relay_url: String::new(),
            host_id: String::new(),
            host_public_key: String::new(),
            host_token: None,
        }
    }
}

impl OnlineSettingsDocument {
    /// The credential actually to present to the relay.
    ///
    /// The environment always wins over the file (`m7-online-networking.md`
    /// §10.1), so a deployment can keep the credential out of the profile
    /// entirely by setting [`RELAY_TOKEN_ENV`] and leaving the field `None`.
    pub fn resolved_host_token(&self) -> Option<String> {
        match std::env::var(RELAY_TOKEN_ENV) {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => self.host_token.clone(),
        }
    }

    /// Whether enough is configured for a connection to be attempted.
    pub fn is_configured(&self) -> bool {
        !self.relay_url.trim().is_empty()
            && !self.host_id.trim().is_empty()
            && self.resolved_host_token().is_some()
    }
}

/// Absolute path of the settings document, creating the app data directory if
/// needed.
pub fn resolve_path(app: &AppHandle) -> Option<PathBuf> {
    let directory = app.path().app_data_dir().ok()?;
    if fs::create_dir_all(&directory).is_err() {
        return None;
    }
    Some(directory.join(ONLINE_SETTINGS_FILE))
}

/// Reads the document.
///
/// A missing, unreadable or corrupted document is reported as the default
/// (disabled, unconfigured) rather than failing, so the application always
/// starts and simply asks the operator to configure online networking.
pub fn load(path: &Path) -> OnlineSettingsDocument {
    let Some(raw) = fs::read_to_string(path).ok() else {
        return OnlineSettingsDocument::default();
    };
    match serde_json::from_str::<OnlineSettingsDocument>(&raw) {
        Ok(mut document) => {
            document.version = 1;
            document
        }
        Err(_) => OnlineSettingsDocument::default(),
    }
}

/// Writes the document atomically, replacing the previous one.
pub fn save(path: &Path, document: &OnlineSettingsDocument) -> Result<(), String> {
    let payload = serde_json::to_string_pretty(document)
        .map_err(|error| format!("The online settings could not be encoded: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, payload)
        .map_err(|error| format!("The online settings could not be written: {error}"))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("The online settings could not be replaced: {error}"))?;
    Ok(())
}

/// Removes the document, so disabling online networking can leave nothing
/// behind when the operator asks for that.
pub fn clear(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "app-timer-online-settings-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch directory");
        path.join(ONLINE_SETTINGS_FILE)
    }

    fn configured() -> OnlineSettingsDocument {
        OnlineSettingsDocument {
            version: 1,
            enabled: true,
            relay_url: "wss://relay.example.test".to_string(),
            host_id: "h-abcdef".to_string(),
            host_public_key: "AAAA".to_string(),
            host_token: Some("credential".to_string()),
        }
    }

    #[test]
    fn a_missing_document_loads_as_disabled() {
        let path = scratch("missing");
        let document = load(&path);
        assert!(!document.enabled);
        assert!(document.relay_url.is_empty());
        assert!(!document.is_configured());
    }

    #[test]
    fn a_corrupt_document_loads_as_disabled() {
        let path = scratch("corrupt");
        fs::write(&path, "{not json").expect("write");
        let document = load(&path);
        assert!(!document.enabled);
        assert!(!document.is_configured());
    }

    #[test]
    fn a_document_round_trips_through_the_file() {
        let path = scratch("round-trip");
        save(&path, &configured()).expect("save");

        let loaded = load(&path);
        assert!(loaded.enabled);
        assert_eq!(loaded.relay_url, "wss://relay.example.test");
        assert_eq!(loaded.host_id, "h-abcdef");
        assert_eq!(loaded.host_public_key, "AAAA");
        assert_eq!(loaded.host_token.as_deref(), Some("credential"));
        assert!(loaded.is_configured());
    }

    #[test]
    fn the_document_never_contains_a_device_token_or_private_key() {
        let path = scratch("no-secrets");
        save(&path, &configured()).expect("save");
        let raw = fs::read_to_string(&path).expect("read");
        assert!(!raw.contains("deviceToken"));
        assert!(!raw.contains("hostPrivateKey"));
        assert!(!raw.contains("privateKey"));
    }

    #[test]
    fn a_host_without_a_relay_url_is_not_configured() {
        let mut document = configured();
        document.relay_url = "   ".to_string();
        assert!(!document.is_configured());
    }

    #[test]
    fn a_host_without_a_credential_is_not_configured() {
        let mut document = configured();
        document.host_token = None;
        // Only meaningful when the environment has not supplied one.
        if std::env::var(RELAY_TOKEN_ENV).is_err() {
            assert!(!document.is_configured());
        }
    }

    #[test]
    fn clearing_removes_the_document() {
        let path = scratch("clear");
        save(&path, &configured()).expect("save");
        assert!(path.exists());
        clear(&path);
        assert!(!path.exists());
    }
}
