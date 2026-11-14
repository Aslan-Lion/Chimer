//! Broadcast helpers for the timer engine.
//!
//! Every timer update reaches both windows through these two channels, so the
//! control window and the display window can never observe different state.
//! The channel names themselves live in [`crate::events`].

use tauri::{AppHandle, Emitter};

use crate::events as channels;

use super::state::TimerSnapshot;

/// Broadcasts the current snapshot over `timer://state-changed`.
pub fn emit_state(app: &AppHandle, snapshot: &TimerSnapshot) {
    let _ = app.emit(channels::TIMER_STATE_CHANGED, snapshot);
}

/// Broadcasts the one-off completion notification over `timer://completed`.
pub fn emit_completed(app: &AppHandle, snapshot: &TimerSnapshot) {
    let _ = app.emit(channels::TIMER_COMPLETED, snapshot);
}
