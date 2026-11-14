//! The wire protocol between the host and a remote browser.
//!
//! Two rules shape everything here:
//!
//! * the host never trusts a field it did not compute. A remote may say *which*
//!   preset it wants to start; it may never say what the timer's remaining time
//!   is, which permissions it holds, or what the current revision is.
//! * every rejection carries a stable code, so the remote interface can explain
//!   the failure instead of showing a generic error.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::display::{PresentationOptions, SessionPresentation};
use crate::session::state::{SessionDefinition, SessionSnapshot};
use crate::timer::state::TimerSnapshot;

use super::model::{DeviceKind, DevicePermission, DeviceRole, PairedDevice};

/// Version of the message vocabulary. A remote that speaks a different version
/// is told so and refused, rather than being fed messages it cannot parse.
pub const PROTOCOL_VERSION: u32 = 1;

/// Longest identifier a remote may send for a preset or a session.
const MAX_IDENTIFIER: usize = 128;

/// Longest timer name a remote may send when configuring a timer.
const MAX_TIMER_NAME: usize = 64;

/// Longest duration a remote may configure: 99 hours.
///
/// The desktop "Create timer" dialog caps the hours field at 99, so the remote
/// path enforces the same ceiling here rather than trusting the browser to.
const MAX_TIMER_DURATION_MS: i64 = 99 * 60 * 60 * 1_000;

/// A failure a remote can be told about.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn malformed(detail: impl Into<String>) -> Self {
        Self::new("malformed", detail)
    }

    pub fn unauthenticated() -> Self {
        Self::new(
            "unauthenticated",
            "This device is not paired. Scan the host's pairing code first.",
        )
    }

    pub fn forbidden(permission: DevicePermission) -> Self {
        Self::new(
            "forbidden",
            format!(
                "The host has not given this device permission to {}.",
                permission.label()
            ),
        )
    }

    pub fn unsupported_protocol(offered: u32) -> Self {
        Self::new(
            "unsupported-protocol",
            format!(
                "This device speaks protocol {offered}, the host speaks {PROTOCOL_VERSION}. Reload the page."
            ),
        )
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

pub type ProtocolResult<T> = Result<T, ProtocolError>;

// ---------------------------------------------------------------------------
// Remote -> host
// ---------------------------------------------------------------------------

/// Everything a remote browser may send.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMessage {
    /// First message on a fresh socket: declares the protocol version only.
    Hello {
        #[serde(default = "default_protocol")]
        protocol: u32,
    },
    /// Redeem a short-lived pairing code.
    Pair {
        code: String,
        #[serde(default)]
        name: Option<String>,
    },
    /// Come back with the device token issued at pairing time.
    Authenticate {
        #[serde(rename = "deviceId")]
        device_id: String,
        token: String,
    },
    /// Run one permitted action.
    Command {
        id: String,
        action: String,
        #[serde(default)]
        payload: Option<Value>,
    },
    /// Ask for the complete current state again (used after a reconnect).
    Snapshot,
    /// Liveness probe.
    Ping {
        #[serde(default)]
        nonce: Option<u64>,
    },
}

fn default_protocol() -> u32 {
    PROTOCOL_VERSION
}

impl ClientMessage {
    /// Parses one text frame.
    ///
    /// Size is checked by the transport before this runs, but it is repeated
    /// here so the parser is safe on its own.
    pub fn parse(text: &str) -> ProtocolResult<ClientMessage> {
        if text.len() > super::model::MAX_MESSAGE_BYTES {
            return Err(ProtocolError::new(
                "too-large",
                "That message is larger than the host accepts.",
            ));
        }
        serde_json::from_str(text).map_err(|error| ProtocolError::malformed(error.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// A validated action, already narrowed to the fixed vocabulary.
///
/// Nothing reaches the engine without passing through here, so the engine can
/// never be handed an arbitrary method name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAction {
    TimerStart,
    TimerPause,
    TimerResume,
    TimerAbort,
    /// Configure a new timer from the remote; it is not started.
    TimerCreate { name: String, duration_ms: i64 },
    PresetStart { preset_id: String },
    SessionStart { session_id: String },
    SessionAdvance,
    SessionStop,
}

impl RemoteAction {
    /// Parses and validates an action plus its payload.
    pub fn parse(action: &str, payload: Option<&Value>) -> ProtocolResult<RemoteAction> {
        match action {
            "timer.start" => Ok(RemoteAction::TimerStart),
            "timer.pause" => Ok(RemoteAction::TimerPause),
            "timer.resume" => Ok(RemoteAction::TimerResume),
            "timer.abort" => Ok(RemoteAction::TimerAbort),
            "timer.create" => Ok(RemoteAction::TimerCreate {
                name: timer_name(payload)?,
                duration_ms: timer_duration(payload)?,
            }),
            "preset.start" => Ok(RemoteAction::PresetStart {
                preset_id: identifier(payload, "presetId")?,
            }),
            "session.start" => Ok(RemoteAction::SessionStart {
                session_id: identifier(payload, "sessionId")?,
            }),
            "session.advance" => Ok(RemoteAction::SessionAdvance),
            "session.stop" => Ok(RemoteAction::SessionStop),
            other => Err(ProtocolError::new(
                "unknown-action",
                format!("'{other}' is not an action this host accepts."),
            )),
        }
    }

    /// The permission a device must hold to run this action.
    pub fn permission(&self) -> DevicePermission {
        match self {
            RemoteAction::TimerStart => DevicePermission::TimerStart,
            RemoteAction::TimerPause => DevicePermission::TimerPause,
            RemoteAction::TimerResume => DevicePermission::TimerResume,
            RemoteAction::TimerAbort => DevicePermission::TimerAbort,
            RemoteAction::TimerCreate { .. } => DevicePermission::TimerCreate,
            RemoteAction::PresetStart { .. } => DevicePermission::PresetStart,
            RemoteAction::SessionStart { .. } => DevicePermission::SessionStart,
            RemoteAction::SessionAdvance => DevicePermission::SessionAdvance,
            RemoteAction::SessionStop => DevicePermission::SessionStop,
        }
    }
}

/// Reads a required identifier out of a payload, refusing anything that is not a
/// short, printable string.
fn identifier(payload: Option<&Value>, field: &str) -> ProtocolResult<String> {
    let missing = || ProtocolError::new("invalid-payload", format!("'{field}' is required."));
    let value = payload.and_then(|value| value.get(field)).ok_or_else(missing)?;
    let text = value.as_str().ok_or_else(missing)?;

    if text.is_empty() || text.len() > MAX_IDENTIFIER {
        return Err(ProtocolError::new(
            "invalid-payload",
            format!("'{field}' is not a valid identifier."),
        ));
    }
    if !text
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err(ProtocolError::new(
            "invalid-payload",
            format!("'{field}' contains characters the host does not accept."),
        ));
    }
    Ok(text.to_string())
}

/// Reads the timer name a remote wants to configure.
///
/// A timer name is free text (spaces are allowed), unlike an identifier, so the
/// check is different: it must be present, printable and short.
fn timer_name(payload: Option<&Value>) -> ProtocolResult<String> {
    let missing = || ProtocolError::new("invalid-payload", "'name' is required.");
    let text = payload
        .and_then(|value| value.get("name"))
        .and_then(|value| value.as_str())
        .ok_or_else(missing)?
        .trim();

    if text.is_empty() {
        return Err(missing());
    }
    if text.chars().count() > MAX_TIMER_NAME || text.chars().any(char::is_control) {
        return Err(ProtocolError::new(
            "invalid-payload",
            format!("'name' must be at most {MAX_TIMER_NAME} printable characters."),
        ));
    }
    Ok(text.to_string())
}

/// Reads the duration a remote wants to configure, in milliseconds.
///
/// The same range the engine and the desktop dialog support: strictly positive
/// and no longer than 99 hours.
fn timer_duration(payload: Option<&Value>) -> ProtocolResult<i64> {
    let missing = || ProtocolError::new("invalid-payload", "'durationMs' is required.");
    let millis = payload
        .and_then(|value| value.get("durationMs"))
        .and_then(Value::as_i64)
        .ok_or_else(missing)?;

    if millis <= 0 || millis > MAX_TIMER_DURATION_MS {
        return Err(ProtocolError::new(
            "invalid-payload",
            format!(
                "The duration must be greater than zero and no more than {} hours.",
                MAX_TIMER_DURATION_MS / 3_600_000
            ),
        ));
    }
    Ok(millis)
}

// ---------------------------------------------------------------------------
// Host -> remote
// ---------------------------------------------------------------------------

/// A paired device as the operator and the device itself see it. The token
/// digest never leaves the host.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDescriptor {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub role: DeviceRole,
    pub permissions: Vec<DevicePermission>,
    pub paired_at: i64,
    pub last_seen_at: i64,
}

impl From<&PairedDevice> for DeviceDescriptor {
    fn from(device: &PairedDevice) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            kind: device.kind,
            role: device.role,
            permissions: device.permissions.clone(),
            paired_at: device.paired_at,
            last_seen_at: device.last_seen_at,
        }
    }
}

impl From<PairedDevice> for DeviceDescriptor {
    fn from(device: PairedDevice) -> Self {
        DeviceDescriptor::from(&device)
    }
}

/// A timer preset the remote controller may start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePreset {
    pub id: String,
    pub name: String,
    pub duration_ms: i64,
}

/// A session the remote controller may start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSession {
    pub id: String,
    pub name: String,
    pub stage_count: usize,
    pub definition: SessionDefinition,
}

/// The part of the operator's library a remote controller needs.
///
/// The library itself lives in the control window; the host publishes a mirror
/// so a remote can list presets and sessions and *ask* for one, which the host
/// then resolves against this same mirror.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryMirror {
    pub presets: Vec<RemotePreset>,
    pub sessions: Vec<RemoteSession>,
}

impl LibraryMirror {
    pub fn find_preset(&self, id: &str) -> Option<&RemotePreset> {
        self.presets.iter().find(|preset| preset.id == id)
    }

    pub fn find_session(&self, id: &str) -> Option<&RemoteSession> {
        self.sessions.iter().find(|session| session.id == id)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// The complete authoritative state, sent once a device is authenticated.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    pub revision: u64,
    pub timer: Option<TimerSnapshot>,
    pub session: Option<SessionSnapshot>,
    pub session_presentation: Option<SessionPresentation>,
    pub presentation: PresentationOptions,
    pub library_revision: u64,
    pub server_time_ms: i64,
}

/// Everything a remote browser may receive.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMessage {
    Welcome {
        protocol: u32,
        server: ServerInfo,
        #[serde(rename = "authenticated")]
        authenticated: bool,
        #[serde(rename = "role")]
        role: Option<DeviceRole>,
        #[serde(rename = "permissions")]
        permissions: Vec<DevicePermission>,
    },
    PairingPending {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "expiresAt")]
        expires_at: i64,
    },
    Paired {
        device: DeviceDescriptor,
        /// The single credential the device keeps. It is presented again on
        /// every reconnect, so no separate session token is issued.
        token: String,
    },
    PairingRejected {
        #[serde(rename = "requestId")]
        request_id: String,
        reason: String,
    },
    DeviceUpdated {
        #[serde(rename = "role")]
        role: DeviceRole,
        #[serde(rename = "permissions")]
        permissions: Vec<DevicePermission>,
    },
    Revoked {
        reason: String,
    },
    Library {
        revision: u64,
        presets: Vec<RemotePreset>,
        sessions: Vec<RemoteSession>,
    },
    Snapshot {
        snapshot: HostSnapshot,
    },
    State {
        revision: u64,
        timer: Option<TimerSnapshot>,
        session: Option<SessionSnapshot>,
        #[serde(rename = "sessionPresentation")]
        session_presentation: Option<SessionPresentation>,
    },
    Presentation {
        revision: u64,
        presentation: PresentationOptions,
    },
    CommandResult {
        id: String,
        ok: bool,
        error: Option<ProtocolError>,
    },
    Error {
        code: String,
        message: String,
    },
    Pong {
        nonce: Option<u64>,
    },
}

impl ServerMessage {
    /// Serialises to the text frame sent over the socket.
    pub fn to_json(&self) -> String {
        // The message is built entirely from serialisable primitives, so this
        // cannot fail; the fallback keeps the transport total.
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"type":"error","code":"internal","message":"The host could not encode that message."}"#
                .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_hello_declares_its_protocol() {
        let message = ClientMessage::parse(r#"{"type":"hello","protocol":1}"#).unwrap();
        assert!(matches!(message, ClientMessage::Hello { protocol: 1 }));
    }

    #[test]
    fn a_hello_without_a_version_defaults_to_this_one() {
        let message = ClientMessage::parse(r#"{"type":"hello"}"#).unwrap();
        assert!(matches!(message, ClientMessage::Hello { protocol } if protocol == PROTOCOL_VERSION));
    }

    #[test]
    fn a_pair_message_carries_the_code() {
        let message =
            ClientMessage::parse(r#"{"type":"pair","code":"123456","name":"Pixel"}"#).unwrap();
        match message {
            ClientMessage::Pair { code, name } => {
                assert_eq!(code, "123456");
                assert_eq!(name.as_deref(), Some("Pixel"));
            }
            other => panic!("expected a pair message, got {other:?}"),
        }
    }

    #[test]
    fn an_authenticate_message_uses_the_camel_case_field() {
        let message =
            ClientMessage::parse(r#"{"type":"authenticate","deviceId":"d-1","token":"t"}"#).unwrap();
        match message {
            ClientMessage::Authenticate { device_id, token } => {
                assert_eq!(device_id, "d-1");
                assert_eq!(token, "t");
            }
            other => panic!("expected an authenticate message, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_is_refused_with_a_stable_code() {
        let error = ClientMessage::parse("{ not json").unwrap_err();
        assert_eq!(error.code, "malformed");
    }

    #[test]
    fn an_unknown_message_type_is_refused() {
        let error = ClientMessage::parse(r#"{"type":"shutdown"}"#).unwrap_err();
        assert_eq!(error.code, "malformed");
    }

    #[test]
    fn an_oversized_message_is_refused_before_parsing() {
        let huge = "x".repeat(super::super::model::MAX_MESSAGE_BYTES + 1);
        let error = ClientMessage::parse(&huge).unwrap_err();
        assert_eq!(error.code, "too-large");
    }

    #[test]
    fn the_simple_actions_need_no_payload() {
        assert_eq!(
            RemoteAction::parse("timer.start", None).unwrap(),
            RemoteAction::TimerStart
        );
        assert_eq!(
            RemoteAction::parse("timer.abort", None).unwrap(),
            RemoteAction::TimerAbort
        );
        assert_eq!(
            RemoteAction::parse("session.advance", None).unwrap(),
            RemoteAction::SessionAdvance
        );
    }

    #[test]
    fn an_unknown_action_is_refused() {
        let error = RemoteAction::parse("timer.explode", None).unwrap_err();
        assert_eq!(error.code, "unknown-action");
    }

    #[test]
    fn an_action_that_needs_a_payload_refuses_to_run_without_one() {
        let error = RemoteAction::parse("preset.start", None).unwrap_err();
        assert_eq!(error.code, "invalid-payload");

        let empty = json!({ "presetId": "" });
        assert_eq!(
            RemoteAction::parse("preset.start", Some(&empty)).unwrap_err().code,
            "invalid-payload"
        );

        let wrong_type = json!({ "presetId": 7 });
        assert_eq!(
            RemoteAction::parse("session.start", Some(&wrong_type)).unwrap_err().code,
            "invalid-payload"
        );
    }

    #[test]
    fn an_identifier_with_path_characters_is_refused() {
        let hostile = json!({ "presetId": "../../etc/passwd" });
        assert_eq!(
            RemoteAction::parse("preset.start", Some(&hostile)).unwrap_err().code,
            "invalid-payload"
        );

        let long = json!({ "presetId": "x".repeat(MAX_IDENTIFIER + 1) });
        assert_eq!(
            RemoteAction::parse("preset.start", Some(&long)).unwrap_err().code,
            "invalid-payload"
        );
    }

    #[test]
    fn a_valid_identifier_is_accepted() {
        let payload = json!({ "sessionId": "session-1729_a.b" });
        assert_eq!(
            RemoteAction::parse("session.start", Some(&payload)).unwrap(),
            RemoteAction::SessionStart {
                session_id: "session-1729_a.b".to_string()
            }
        );
    }

    #[test]
    fn every_action_maps_to_a_permission() {
        assert_eq!(
            RemoteAction::TimerStart.permission(),
            DevicePermission::TimerStart
        );
        assert_eq!(
            RemoteAction::PresetStart {
                preset_id: "p".to_string()
            }
            .permission(),
            DevicePermission::PresetStart
        );
        assert_eq!(
            RemoteAction::TimerCreate {
                name: "Exam".to_string(),
                duration_ms: 60_000,
            }
            .permission(),
            DevicePermission::TimerCreate
        );
        assert_eq!(
            RemoteAction::SessionStop.permission(),
            DevicePermission::SessionStop
        );
    }

    #[test]
    fn a_timer_create_action_carries_a_name_and_a_duration() {
        let payload = json!({ "name": "Morning examination", "durationMs": 1_500_000 });
        assert_eq!(
            RemoteAction::parse("timer.create", Some(&payload)).unwrap(),
            RemoteAction::TimerCreate {
                name: "Morning examination".to_string(),
                duration_ms: 1_500_000,
            }
        );
    }

    #[test]
    fn a_timer_create_without_a_payload_is_refused() {
        let error = RemoteAction::parse("timer.create", None).unwrap_err();
        assert_eq!(error.code, "invalid-payload");
    }

    #[test]
    fn a_timer_create_with_an_empty_or_oversized_name_is_refused() {
        for name in ["", "   ", &"x".repeat(MAX_TIMER_NAME + 1)] {
            let payload = json!({ "name": name, "durationMs": 60_000 });
            assert_eq!(
                RemoteAction::parse("timer.create", Some(&payload)).unwrap_err().code,
                "invalid-payload",
                "name {name:?} must be refused"
            );
        }
    }

    #[test]
    fn a_timer_create_with_a_non_positive_or_oversized_duration_is_refused() {
        for duration in [0_i64, -1, -60_000, MAX_TIMER_DURATION_MS + 1] {
            let payload = json!({ "name": "Exam", "durationMs": duration });
            assert_eq!(
                RemoteAction::parse("timer.create", Some(&payload)).unwrap_err().code,
                "invalid-payload",
                "duration {duration} must be refused"
            );
        }

        let wrong_type = json!({ "name": "Exam", "durationMs": "60000" });
        assert_eq!(
            RemoteAction::parse("timer.create", Some(&wrong_type)).unwrap_err().code,
            "invalid-payload"
        );
    }

    #[test]
    fn a_timer_create_accepts_the_full_supported_range() {
        let bounds = [
            1_i64,
            1_000,
            MAX_TIMER_DURATION_MS,
        ];
        for duration in bounds {
            let payload = json!({ "name": "Exam", "durationMs": duration });
            assert_eq!(
                RemoteAction::parse("timer.create", Some(&payload)).unwrap(),
                RemoteAction::TimerCreate {
                    name: "Exam".to_string(),
                    duration_ms: duration,
                }
            );
        }
    }

    #[test]
    fn a_welcome_message_serialises_with_a_type_tag() {
        let json = ServerMessage::Welcome {
            protocol: PROTOCOL_VERSION,
            server: ServerInfo {
                name: "app.timer".to_string(),
                version: "0.1.0".to_string(),
            },
            authenticated: false,
            role: None,
            permissions: Vec::new(),
        }
        .to_json();
        assert!(json.contains(r#""type":"welcome""#));
        assert!(json.contains(r#""authenticated":false"#));
    }

    #[test]
    fn a_device_descriptor_hides_the_token_digest() {
        let device = PairedDevice {
            id: "d-1".to_string(),
            name: "Pixel".to_string(),
            kind: DeviceKind::Phone,
            role: DeviceRole::Controller,
            permissions: vec![DevicePermission::TimerStart],
            token_hash: "deadbeef".to_string(),
            paired_at: 1,
            last_seen_at: 2,
            user_agent: "ua".to_string(),
            device_static_pk: None,
        };
        let json = serde_json::to_string(&DeviceDescriptor::from(&device)).unwrap();
        assert!(!json.contains("deadbeef"), "the digest must never leave the host");
        assert!(json.contains("Pixel"));
    }

    #[test]
    fn the_library_mirror_resolves_by_id() {
        let mirror = LibraryMirror {
            presets: vec![RemotePreset {
                id: "p-1".to_string(),
                name: "Exam".to_string(),
                duration_ms: 60_000,
            }],
            sessions: Vec::new(),
        };
        assert_eq!(mirror.find_preset("p-1").map(|p| p.name.as_str()), Some("Exam"));
        assert!(mirror.find_preset("p-2").is_none());
    }
}
