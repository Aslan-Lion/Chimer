//! Event channel names shared by the Rust engine and every webview.
//!
//! Keeping the names in one place means the control window and the display
//! window can never drift apart on spelling. The channels are broadcast once
//! the timer engine and the display window are implemented.

#![allow(dead_code)]

/// Emitted whenever the authoritative timer state changes.
pub const TIMER_STATE_CHANGED: &str = "timer://state-changed";

/// Emitted once when a countdown reaches zero. Kept separate from
/// [`TIMER_STATE_CHANGED`] so notification and sound options can subscribe to
/// completion alone without filtering every tick.
pub const TIMER_COMPLETED: &str = "timer://completed";

/// Emitted when the display window opens, closes, moves or changes mode.
pub const DISPLAY_WINDOW_STATE_CHANGED: &str = "display://state-changed";

/// Emitted when the set of connected monitors changes.
pub const MONITORS_CHANGED: &str = "display://monitors-changed";

/// Emitted with the physical placement of the display window so the control
/// window can remember it across open/close cycles.
pub const DISPLAY_WINDOW_GEOMETRY: &str = "display://geometry";

/// Emitted when the shared presentation options change. The display window
/// listens to this and the timer engine reuses the same channel in Milestone 3.
pub const DISPLAY_PRESENTATION_CHANGED: &str = "display://presentation-changed";

/// Emitted when the session engine changes the stage shown on the display
/// window. Payload is a `SessionPresentation` or `null` when no session runs.
pub const DISPLAY_SESSION_CHANGED: &str = "display://session-changed";

/// Emitted whenever the session cursor changes: a stage starts, a stage
/// finishes, the session waits for the operator or the whole session completes.
pub const SESSION_STATE_CHANGED: &str = "session://state-changed";

/// Emitted for the discrete moments a session should notify about — a stage
/// starting, a warning threshold being crossed, a stage finishing and the
/// session finishing. Kept separate from [`SESSION_STATE_CHANGED`] so the
/// frontend can play a sound exactly once per cue.
pub const SESSION_CUE: &str = "session://cue";

/// Emitted when the local server starts, stops, fails to bind or gains or
/// loses a connection. Payload is a `NetworkStatus`.
pub const NETWORK_STATUS_CHANGED: &str = "network://status-changed";

/// Emitted when a device is paired, revoked, reconnects or changes role or
/// permissions. Payload is the list of `DeviceView`s.
pub const NETWORK_DEVICES_CHANGED: &str = "network://devices-changed";

/// Emitted when a device enters a pairing code and waits for the operator, when
/// the operator decides, or when a request expires. Payload is the list of
/// pending `PairingRequest`s.
pub const NETWORK_PAIRING_CHANGED: &str = "network://pairing-changed";
