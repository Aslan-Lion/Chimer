//! Session IPC commands.
//!
//! Thin adapters over [`SessionEngine`], mirroring [`crate::timer::commands`]:
//! they translate the argument shapes and hand the engine the application
//! handle it needs to broadcast. Every command returns the authoritative cursor
//! *and* broadcasts it, so both windows converge on the same stage.
//!
//! None of these commands creates or modifies a native window, so they run
//! synchronously on the main thread — only commands that build windows have to
//! be `async`.

use tauri::{AppHandle, State};

use super::engine::SessionEngine;
use super::state::{SessionDefinition, SessionSnapshot};

/// Reads the current authoritative session cursor.
///
/// The display window calls this when it opens so it can never render a stale
/// stage, no matter when it was launched relative to the session.
#[tauri::command]
pub fn session_snapshot(engine: State<'_, SessionEngine>) -> SessionSnapshot {
    engine.current()
}

/// Loads a session and starts its first stage.
#[tauri::command]
pub fn session_start(
    app: AppHandle,
    engine: State<'_, SessionEngine>,
    definition: SessionDefinition,
) -> Result<SessionSnapshot, String> {
    engine.start(&app, definition)
}

/// Advances to the next stage — the operator's action at a manual transition,
/// or a deliberate skip of a transition countdown.
#[tauri::command]
pub fn session_next(
    app: AppHandle,
    engine: State<'_, SessionEngine>,
) -> Result<SessionSnapshot, String> {
    engine.next(&app)
}

/// Ends the session and returns both windows to the idle presentation.
#[tauri::command]
pub fn session_stop(
    app: AppHandle,
    engine: State<'_, SessionEngine>,
) -> Result<SessionSnapshot, String> {
    engine.stop(&app)
}
