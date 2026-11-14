//! Authoritative countdown engine.
//!
//! Rust owns every timer value: the configured duration, the current state, the
//! live remaining time, the progress and the overtime counter. Both the control
//! window and the display window render the same broadcast
//! [`TimerSnapshot`] and never derive a countdown of their own — there is no
//! `setInterval` involved anywhere in the application.
//!
//! * [`state`] — the clock-injected state machine and its transition rules
//! * [`engine`] — the managed owner that locks, persists and broadcasts
//! * [`commands`] — the IPC surface shared by both webviews
//! * [`persistence`] — restart recovery for the runtime state
//! * [`events`] — the `timer://` broadcast helpers

pub mod commands;
pub mod engine;
pub mod events;
pub mod persistence;
pub mod state;

pub use engine::TimerEngine;
