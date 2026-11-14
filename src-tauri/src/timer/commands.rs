//! Timer IPC commands.
//!
//! Thin adapters over [`TimerEngine`]: they only translate argument shapes and
//! hand the engine the application handle it needs to broadcast the result.
//! Every command returns the authoritative snapshot *and* broadcasts it, so the
//! calling window and the display window always converge on the same value.
//!
//! None of these commands creates or modifies a native window, so they run
//! synchronously on the main thread. Only commands that build windows (see
//! [`crate::commands::open_display_window`]) have to be `async`.

use tauri::{AppHandle, State};

use super::engine::TimerEngine;
use super::state::TimerSnapshot;

/// Reads the current authoritative snapshot.
///
/// The display window calls this when it opens so it can never render a stale
/// countdown, no matter when it was launched relative to the timer.
#[tauri::command]
pub fn timer_snapshot(engine: State<'_, TimerEngine>) -> TimerSnapshot {
    engine.current()
}

/// Configures a new timer. Replaces any previous timer, because only one timer
/// can ever be active.
#[tauri::command]
pub fn timer_create(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
    name: String,
    duration_ms: i64,
) -> Result<TimerSnapshot, String> {
    engine.create(&app, name, duration_ms)
}

/// Starts the countdown.
#[tauri::command]
pub fn timer_start(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
) -> Result<TimerSnapshot, String> {
    engine.start(&app)
}

/// Freezes the countdown, preserving the remaining time.
#[tauri::command]
pub fn timer_pause(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
) -> Result<TimerSnapshot, String> {
    engine.pause(&app)
}

/// Continues a paused countdown from where it stopped.
#[tauri::command]
pub fn timer_resume(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
) -> Result<TimerSnapshot, String> {
    engine.resume(&app)
}

/// Restores the original duration and returns the timer to `Idle`.
#[tauri::command]
pub fn timer_reset(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
) -> Result<TimerSnapshot, String> {
    engine.reset(&app)
}

/// Aborts the timer, discarding it and returning the engine to the empty
/// `Idle` state.
///
/// Unlike [`timer_reset`], which keeps the timer configured at its full
/// duration, this is a genuine engine reset of the whole timer: both windows
/// fall back to the idle presentation (`00:00`, zero progress, no overtime
/// counter) and the cleared state is persisted.
#[tauri::command]
pub fn timer_abort(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
) -> Result<TimerSnapshot, String> {
    engine.abort(&app)
}

/// Terminates the countdown, preserving the remaining time.
#[tauri::command]
pub fn timer_stop(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
) -> Result<TimerSnapshot, String> {
    engine.stop(&app)
}

/// Adds or subtracts milliseconds from a running, paused or completed timer.
#[tauri::command]
pub fn timer_adjust(
    app: AppHandle,
    engine: State<'_, TimerEngine>,
    delta_ms: i64,
) -> Result<TimerSnapshot, String> {
    engine.adjust(&app, delta_ms)
}
