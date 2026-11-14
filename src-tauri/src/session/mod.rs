//! Sequential session execution (Milestone 4).
//!
//! A *session* is an ordered list of stages. Each stage is an ordinary
//! countdown, so the session never grows a second timer: every stage is run by
//! the existing authoritative [`crate::timer`] engine, and this module only
//! decides *which* stage is running and *when* the next one begins.
//!
//! * [`state`] — the pure, clock-injected session cursor and its transitions
//! * [`engine`] — the managed owner that settles stages, persists and broadcasts
//! * [`commands`] — the IPC surface shared by both webviews
//! * [`events`] — the broadcast channels
//! * [`persistence`] — restart recovery for the session cursor
//!
//! Keeping the cursor here rather than in the frontend is what lets a session
//! survive a restart and lets the display window follow the stages without the
//! control window being open.

pub mod commands;
pub mod engine;
pub mod events;
pub mod persistence;
pub mod state;
