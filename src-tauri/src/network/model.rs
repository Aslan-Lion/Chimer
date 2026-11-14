//! Vocabulary shared by the local network engine, its IPC surface and the
//! remote web client.
//!
//! Nothing in this module knows about sockets or about the timer: it only names
//! the concepts a remote device is described by, so the server, the persistence
//! layer and the frontend all speak the same language.

use serde::{Deserialize, Serialize};

/// Default port for the local server.
///
/// Above 1024 so binding never requires administrator rights on Windows, and
/// outside the ranges commonly grabbed by development tooling.
pub const DEFAULT_PORT: u16 = 8722;

/// Lowest port the interface will accept. Below this, Windows requires
/// elevation to bind a listener.
pub const MIN_PORT: u16 = 1024;

/// Most remote connections the host accepts at once.
pub const MAX_CONNECTIONS: usize = 16;

/// Largest client frame the server will read, in bytes.
///
/// A remote client only ever sends a small JSON command, so anything larger is
/// either a bug or an attempt to exhaust memory. The limit is enforced while
/// reading the frame, before the payload is buffered.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// Largest number of devices that may be paired at once.
pub const MAX_PAIRED_DEVICES: usize = 32;

/// How long a pairing code stays valid.
pub const PAIRING_TTL_MS: i64 = 120_000;

/// How long a pairing request waits for the operator's decision.
pub const PAIRING_REQUEST_TTL_MS: i64 = 120_000;

/// Digits in a pairing code.
pub const PAIRING_CODE_DIGITS: usize = 6;

/// Bytes of entropy in a device token (192 bits, base64url encoded).
///
/// This single token is what a device keeps to re-authenticate: a reconnect
/// simply presents it again, so no second per-connection secret is issued.
pub const DEVICE_TOKEN_BYTES: usize = 24;

/// Pairing attempts allowed from one address inside [`PAIRING_WINDOW_MS`].
pub const PAIRING_ATTEMPTS: usize = 5;
pub const PAIRING_WINDOW_MS: i64 = 5 * 60_000;

/// Commands a single connection may issue per second.
pub const COMMAND_BURST: usize = 20;
pub const COMMAND_WINDOW_MS: i64 = 1_000;

/// How many recently executed command ids are remembered per device, so a
/// retried command is answered from the cache instead of being run twice.
pub const COMMAND_MEMORY: usize = 64;

/// What a paired device is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
    /// May issue the commands the operator has granted.
    Controller,
    /// Read-only: receives the presentation and can never control the timer.
    Display,
}

impl DeviceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceRole::Controller => "controller",
            DeviceRole::Display => "display",
        }
    }

    /// Permissions a freshly paired device of this role starts with.
    ///
    /// A controller is granted the full set; the operator can then narrow it.
    /// A display is granted nothing, because it never issues commands.
    pub fn default_permissions(self) -> Vec<DevicePermission> {
        match self {
            DeviceRole::Controller => DevicePermission::all().to_vec(),
            DeviceRole::Display => Vec::new(),
        }
    }
}

/// An individual capability that can be granted or withheld per device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DevicePermission {
    TimerStart,
    TimerPause,
    TimerResume,
    TimerAbort,
    /// Configure a brand-new timer (name + duration) from the remote device.
    TimerCreate,
    PresetStart,
    SessionStart,
    SessionAdvance,
    SessionStop,
}

impl DevicePermission {
    pub fn all() -> &'static [DevicePermission] {
        &[
            DevicePermission::TimerStart,
            DevicePermission::TimerPause,
            DevicePermission::TimerResume,
            DevicePermission::TimerAbort,
            DevicePermission::TimerCreate,
            DevicePermission::PresetStart,
            DevicePermission::SessionStart,
            DevicePermission::SessionAdvance,
            DevicePermission::SessionStop,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DevicePermission::TimerStart => "timerStart",
            DevicePermission::TimerPause => "timerPause",
            DevicePermission::TimerResume => "timerResume",
            DevicePermission::TimerAbort => "timerAbort",
            DevicePermission::TimerCreate => "timerCreate",
            DevicePermission::PresetStart => "presetStart",
            DevicePermission::SessionStart => "sessionStart",
            DevicePermission::SessionAdvance => "sessionAdvance",
            DevicePermission::SessionStop => "sessionStop",
        }
    }

    /// Human-readable label, used in the rejection message a remote receives.
    pub fn label(self) -> &'static str {
        match self {
            DevicePermission::TimerStart => "start a timer",
            DevicePermission::TimerPause => "pause a timer",
            DevicePermission::TimerResume => "resume a timer",
            DevicePermission::TimerAbort => "abort a timer",
            DevicePermission::TimerCreate => "create or configure a timer",
            DevicePermission::PresetStart => "start a timer preset",
            DevicePermission::SessionStart => "start a session",
            DevicePermission::SessionAdvance => "advance a session",
            DevicePermission::SessionStop => "stop a session",
        }
    }
}

/// Broad device family, inferred from the browser's user agent on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Phone,
    Tablet,
    Desktop,
    Unknown,
}

impl DeviceKind {
    /// Derives the family from a user agent string.
    ///
    /// Deliberately coarse: the value is shown to the operator so they can
    /// recognise the device, never trusted for anything functional.
    pub fn from_user_agent(user_agent: &str) -> DeviceKind {
        let agent = user_agent.to_ascii_lowercase();
        if agent.is_empty() {
            return DeviceKind::Unknown;
        }
        if agent.contains("ipad") || (agent.contains("android") && !agent.contains("mobile")) {
            return DeviceKind::Tablet;
        }
        if agent.contains("iphone")
            || agent.contains("ipod")
            || (agent.contains("android") && agent.contains("mobile"))
            || agent.contains("windows phone")
        {
            return DeviceKind::Phone;
        }
        if agent.contains("mozilla") || agent.contains("applewebkit") || agent.contains("chrome") {
            return DeviceKind::Desktop;
        }
        DeviceKind::Unknown
    }
}

/// A device the operator has approved. Persisted; the token is only ever kept
/// as a hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub role: DeviceRole,
    pub permissions: Vec<DevicePermission>,
    /// SHA-256 of the device token, hex encoded. Never the token itself.
    pub token_hash: String,
    pub paired_at: i64,
    pub last_seen_at: i64,
    pub user_agent: String,
    /// The device's static public key (X25519, 32 bytes, base64url without padding)
    /// for online end-to-end sessions (§12.1). `None` for local-only devices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_static_pk: Option<String>,
}

impl PairedDevice {
    pub fn has(&self, permission: DevicePermission) -> bool {
        // A display is read-only by construction: even if a permission were
        // somehow left on the record, the role wins.
        self.role == DeviceRole::Controller && self.permissions.contains(&permission)
    }
}

/// Which socket a connection arrived on.
///
/// The discriminator is never taken from the peer: it is set by the code path
/// that registered the connection — the M6 LAN listener registers
/// [`Transport::Local`], the relay tunnel registers [`Transport::Online`]. A
/// connection can never change transport, and the online path is inert until
/// its mandatory end-to-end handshake completes. See
/// `m7-e2e-security-design.md` §12.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// The M6 local-network listener (`ws://<lan-ip>:8722`). Not encrypted;
    /// protected by the trusted-LAN model, exactly as in M6.
    Local,
    /// A connection carried by the relay tunnel. Mandatorily sealed end to end.
    Online,
}

/// A remote connection the host is currently holding.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedDevice {
    pub connection_id: String,
    /// `None` until the connection has authenticated.
    pub device_id: Option<String>,
    pub name: String,
    pub kind: DeviceKind,
    pub role: Option<DeviceRole>,
    pub permissions: Vec<DevicePermission>,
    /// Peer address, as reported by the socket.
    pub address: String,
    /// Which leg this connection arrived on.
    pub transport: Transport,
    pub authenticated: bool,
    pub connected_at: i64,
    pub last_seen_at: i64,
    pub user_agent: String,
}

/// A device that has entered a pairing code and is waiting for the operator.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub request_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub address: String,
    pub user_agent: String,
    /// Which leg the request arrived on, so the operator knows where the
    /// device is coming from.
    pub transport: Transport,
    pub requested_at: i64,
    pub expires_at: i64,
    /// The short authentication string (SAS, 8 digits, `dddd dddd`) for online
    /// pairing (§7.3, §7.4). `None` for local LAN pairings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sas: Option<String>,
    /// Fingerprint of the device static key `D_pub` (§7.4, §8.1).
    /// `None` for local LAN pairings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_fingerprint: Option<String>,
}

/// One network interface the host could be reached on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAddress {
    pub address: String,
    /// Interface name, for the operator's benefit.
    pub label: String,
    /// True for RFC 1918 / link-local ranges, which is what a local network is.
    pub is_private: bool,
    /// True when the interface looks like it belongs to a virtual adapter.
    pub is_virtual: bool,
}

impl NetworkAddress {
    /// Sort key: private addresses first, then physical before virtual.
    pub fn rank(&self) -> u8 {
        match (self.is_private, self.is_virtual) {
            (true, false) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (false, true) => 3,
        }
    }
}

/// A failure the operator needs to understand, with an explanation they can act
/// on. `code` is stable so the frontend can pick the matching guidance.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkError {
    pub code: String,
    pub message: String,
}

impl NetworkError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

/// Everything the Settings page needs to describe the server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    /// Whether the operator asked for networking to be on.
    pub enabled: bool,
    /// Whether a listener is actually bound right now.
    pub running: bool,
    /// Address the listener is bound to, or `None` while stopped.
    pub bound_address: Option<String>,
    pub port: u16,
    /// Address a remote device should use, e.g. `http://192.168.1.20:8722`.
    pub url: Option<String>,
    /// Address the user picked, or `None` for "the best local address".
    pub preferred_address: Option<String>,
    /// True when the listener is bound to every interface.
    pub bind_all: bool,
    pub addresses: Vec<NetworkAddress>,
    pub connected_count: usize,
    pub paired_count: usize,
    pub error: Option<NetworkError>,
}
