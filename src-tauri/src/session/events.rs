//! Broadcast helpers for the session engine.
//!
//! Every cursor change reaches both windows through these channels, so the
//! control window and the display window can never disagree about which stage
//! is running. The channel names themselves live in [`crate::events`].

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::events as channels;

use super::state::SessionSnapshot;

/// A discrete moment worth notifying the operator about.
///
/// The frontend turns [`Self::sound`] into an actual sound; Rust only decides
/// *when* the cue happens, keeping the schedule authoritative.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCue {
    /// `stage-start`, `warning`, `stage-complete` or `session-complete`.
    pub kind: &'static str,
    pub session_id: String,
    pub session_name: String,
    pub stage_index: usize,
    pub stage_count: usize,
    pub stage_name: String,
    /// Remaining milliseconds when the cue fired, for warnings.
    pub remaining_ms: Option<i64>,
    /// Whether the operator asked for a sound at this moment.
    pub sound: bool,
}

/// Broadcasts the current cursor over `session://state-changed`.
pub fn emit_state(app: &AppHandle, snapshot: &SessionSnapshot) {
    let _ = app.emit(channels::SESSION_STATE_CHANGED, snapshot);
}

/// Broadcasts one notification over `session://cue`.
pub fn emit_cue(app: &AppHandle, cue: &SessionCue) {
    let _ = app.emit(channels::SESSION_CUE, cue);
}
