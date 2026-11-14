//! IPC commands exposed to the control and display windows.
//!
//! These wrappers are intentionally thin: they only adapt the argument shapes
//! and delegate to the owning module, so the real logic stays testable and the
//! frontend never talks to the window registry directly.

use serde::Serialize;
use tauri::AppHandle;

use crate::display::{
    self, DisplayWindowState, MonitorDescriptor, OpenDisplayOptions, PresentationOptions,
    SessionPresentation,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub runtime: String,
}

/// Metadata surfaced on the Settings page so the frontend can confirm the
/// IPC bridge to Rust is alive.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "app.timer".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        runtime: "tauri-2".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Display window (Milestone 2)
// ---------------------------------------------------------------------------

/// Enumerates every display Windows currently reports.
#[tauri::command]
pub fn list_monitors(app: AppHandle) -> Vec<MonitorDescriptor> {
    display::list_monitors(&app)
}

/// Re-reads the connected displays and returns the fresh list.
#[tauri::command]
pub fn sync_display(app: AppHandle) -> Vec<MonitorDescriptor> {
    display::sync_display(&app)
}

/// Current lifecycle state of the presentation window.
#[tauri::command]
pub fn display_window_state(app: AppHandle) -> DisplayWindowState {
    display::window_state(&app)
}

/// Opens the presentation window, or brings the existing one forward.
///
/// Declared `async` on purpose: Tauri runs a synchronous command on the main
/// thread — which here is the thread currently executing this webview's IPC
/// callback. Creating a *second* WebView2 environment/controller from inside
/// that callback re-enters the WebView2 runtime on the same thread and never
/// completes. An async command body runs on the async runtime instead, so
/// `WebviewWindowBuilder::build()` is dispatched to the event loop from a
/// separate thread, which is the path tauri-runtime-wry is written for.
#[tauri::command]
pub async fn open_display_window(
    app: AppHandle,
    options: OpenDisplayOptions,
) -> Result<DisplayWindowState, String> {
    display::open_display_window(&app, options)
}

/// Closes the presentation window without touching the control window.
#[tauri::command]
pub fn close_display_window(app: AppHandle) -> Result<DisplayWindowState, String> {
    display::close_display_window(&app)
}

/// Toggles the presentation window between windowed and fullscreen modes.
#[tauri::command]
pub fn set_display_fullscreen(
    app: AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    display::set_display_fullscreen(&app, enabled)
}

/// Returns the presentation window to a visible, usable windowed state.
#[tauri::command]
pub fn restore_display_window(app: AppHandle) -> Result<DisplayWindowState, String> {
    display::restore_display_window(&app)
}

/// Pins or unpins the presentation window above other windows.
#[tauri::command]
pub fn set_display_always_on_top(
    app: AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    display::set_display_always_on_top(&app, enabled)
}

/// Maximises the presentation window, or returns it to its restored size.
#[tauri::command]
pub fn set_display_maximized(
    app: AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    display::set_display_maximized(&app, enabled)
}

/// Enters or leaves presentation mode on the existing display window.
#[tauri::command]
pub fn set_display_presentation(
    app: AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    display::set_display_presentation(&app, enabled)
}

/// Moves (or opens) the presentation window on the requested monitor.
///
/// Async for the same reason as [`open_display_window`]: this command falls
/// back to creating the window when it is not open yet.
#[tauri::command]
pub async fn move_display_to_monitor(
    app: AppHandle,
    monitor_key: String,
) -> Result<DisplayWindowState, String> {
    display::move_display_to_monitor(&app, &monitor_key)
}

/// Stores and broadcasts the shared presentation options.
#[tauri::command]
pub fn set_presentation_options(
    app: AppHandle,
    options: PresentationOptions,
) -> PresentationOptions {
    display::set_presentation_options(&app, options)
}

/// Reads the last presentation options pushed by the control window.
#[tauri::command]
pub fn presentation_options(app: AppHandle) -> PresentationOptions {
    display::presentation_options(&app)
}

/// Reads the stage the session engine last pushed, or `None` when no session
/// is running. The display window calls this on startup so a restored session
/// is rendered immediately.
#[tauri::command]
pub fn session_presentation(app: AppHandle) -> Option<SessionPresentation> {
    display::session_presentation(&app)
}
