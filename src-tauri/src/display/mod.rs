//! Native presentation window management.
//!
//! The display window is a genuine, independently movable operating-system
//! window created through the Tauri window API — never a modal, overlay or
//! iframe. Every mutation (open, close, move, fullscreen, restore) is performed
//! here in Rust so the control window can never disagree with the real window
//! registry.
//!
//! Rust also owns the *shared* presentation state (the timer format, the message
//! currently on screen and the complete display style — layout, typography and
//! colours). The control window pushes it through [`set_presentation_options`];
//! the display window reads it back with [`presentation_options`] and listens
//! for [`events::DISPLAY_PRESENTATION_CHANGED`]. This is the same command +
//! broadcast pattern the timer engine reuses in Milestone 3.
//!
//! The style is styling, not state: Rust stores and re-broadcasts it untouched
//! and never interprets it. It is mirrored here only so both sides share one
//! contract, exactly as [`DisplayMessage`] and [`SessionPresentation`] are.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::events;

/// Label of the presentation window. The frontend `AppRouter` uses the same
/// value to decide which interface a webview should mount.
pub const DISPLAY_WINDOW_LABEL: &str = "display";

/// Label of the administrative window declared in `tauri.conf.json`.
pub const CONTROL_WINDOW_LABEL: &str = "main";

/// Smallest usable presentation window, in logical pixels.
const MIN_WIDTH: f64 = 360.0;
const MIN_HEIGHT: f64 = 240.0;

/// Geometry notifications raised while a user drags the window are throttled so
/// the event channel never floods. The exact resting geometry is always capture
/// when the window is closed.
const GEOMETRY_THROTTLE: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Serialised shapes shared with the TypeScript layer
// ---------------------------------------------------------------------------

/// Normalised monitor description returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDescriptor {
    /// Stable identity: monitor name plus its virtual-desktop origin.
    pub key: String,
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub scale_factor: f64,
    pub is_primary: bool,
    /// True for the monitor that currently hosts the control window.
    pub is_current: bool,
}

/// Coarse lifecycle state of the presentation window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayWindowState {
    pub open: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    /// Whether the window is pinned above other windows.
    pub always_on_top: bool,
    /// True while presentation mode is active: fullscreen, always-on-top and a
    /// chrome-free frame, entered and left from the control window.
    pub presentation: bool,
    pub monitor_key: Option<String>,
    pub monitor_name: Option<String>,
}

impl DisplayWindowState {
    fn closed() -> Self {
        Self {
            open: false,
            fullscreen: false,
            maximized: false,
            always_on_top: false,
            presentation: false,
            monitor_key: None,
            monitor_name: None,
        }
    }
}

/// Physical placement of the presentation window, used to remember it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub monitor_key: Option<String>,
}

/// A message preset prepared for presentation.
///
/// `font` and `animation` are the message library's typography and motion
/// selections. They default, so a payload written before the library existed
/// (or one that omits them) still deserialises.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayMessage {
    pub id: String,
    pub name: String,
    pub text: String,
    pub font_size: f64,
    pub color: String,
    pub background_color: Option<String>,
    pub position: String,
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub animation: String,
    /// How long the message stays on screen, in milliseconds; `0` is sticky.
    #[serde(default)]
    pub duration_ms: i64,
}

/// The stage a running session is currently on, pushed to the display window.
///
/// Like [`PresentationOptions`], Rust owns this: the session engine writes it
/// from its authoritative cursor, so the display window can render the current
/// stage without the control window having to forward anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPresentation {
    pub session_id: String,
    pub session_name: String,
    /// Zero-based index of the running stage.
    pub stage_index: usize,
    pub stage_count: usize,
    pub stage_name: String,
    /// True while the operator must start the next stage manually.
    pub awaiting_manual: bool,
    /// Remaining time of an automatic transition countdown, if one is running.
    pub transition_countdown_ms: Option<i64>,
    /// The message the current stage shows, if it has one.
    pub message: Option<DisplayMessage>,
}

/// Typography for one presentation text element.
///
/// `scale` multiplies the element's responsive base size rather than replacing
/// it, so a configuration that reads well on an external 4K panel still fits a
/// laptop screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayTextStyle {
    /// Font family key: `sans`, `mono`, `serif` or `display`.
    pub font: String,
    /// Multiplier applied to the element's responsive base size.
    pub scale: f64,
    /// CSS font weight.
    pub weight: i64,
    pub align: String,
}

impl DisplayTextStyle {
    fn new(font: &str) -> Self {
        Self {
            font: font.to_string(),
            scale: 1.0,
            weight: 600,
            align: "center".to_string(),
        }
    }
}

/// Where each presentation element sits and what is shown at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayLayoutConfig {
    pub timer_position: String,
    pub message_position: String,
    pub progress_position: String,
    pub overtime_corner: String,
    pub show_session_info: bool,
    pub show_stage_info: bool,
    pub show_timer_name: bool,
    pub show_progress_bar: bool,
    pub show_messages: bool,
    /// Vertical rhythm multiplier applied to the gaps between elements.
    pub spacing: f64,
    /// Horizontal alignment of the whole presentation.
    pub align: String,
}

impl Default for DisplayLayoutConfig {
    fn default() -> Self {
        Self {
            timer_position: "center".to_string(),
            message_position: "bottom".to_string(),
            progress_position: "bottom".to_string(),
            overtime_corner: "bottom-right".to_string(),
            show_session_info: false,
            show_stage_info: true,
            show_timer_name: true,
            show_progress_bar: true,
            show_messages: true,
            spacing: 1.0,
            align: "center".to_string(),
        }
    }
}

/// Colours and background of the presentation surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayThemeConfig {
    pub background: String,
    /// Optional background image, held as a data URL, or `null` for none.
    pub background_image: Option<String>,
    pub timer_color: String,
    /// Message text colour. Empty means "leave each message in its own colour".
    pub message_color: String,
    pub progress_color: String,
    pub overtime_color: String,
}

impl Default for DisplayThemeConfig {
    fn default() -> Self {
        Self {
            background: "#050505".to_string(),
            background_image: None,
            timer_color: "#FFFFFF".to_string(),
            message_color: String::new(),
            progress_color: "#8B9DFF".to_string(),
            overtime_color: "#FF4545".to_string(),
        }
    }
}

/// The complete customisation of the presentation surface.
///
/// Styling, not state: Rust stores and re-broadcasts it untouched and never
/// interprets it. The shape is mirrored by `DisplayStyle` in
/// `src/types/display.ts` so both sides share one contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayStyle {
    pub layout: DisplayLayoutConfig,
    pub timer: DisplayTextStyle,
    /// Separate typography for messages, so the two never have to share a size.
    pub message_text: DisplayTextStyle,
    pub theme: DisplayThemeConfig,
}

impl Default for DisplayStyle {
    fn default() -> Self {
        Self {
            layout: DisplayLayoutConfig::default(),
            // The digits are a monospaced, tabular readout; messages are prose.
            timer: DisplayTextStyle::new("mono"),
            message_text: DisplayTextStyle::new("sans"),
            theme: DisplayThemeConfig::default(),
        }
    }
}

/// Everything the display window needs to render itself. Kept free of timer
/// data on purpose — the timer engine adds its own snapshot in Milestone 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationOptions {
    pub timer_format: String,
    #[serde(default)]
    pub message: Option<DisplayMessage>,
    #[serde(default)]
    pub style: DisplayStyle,
}

impl Default for PresentationOptions {
    fn default() -> Self {
        Self {
            timer_format: "hh:mm:ss".to_string(),
            message: None,
            style: DisplayStyle::default(),
        }
    }
}

/// Arguments accepted by the `open_display_window` command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDisplayOptions {
    #[serde(default)]
    pub monitor_key: Option<String>,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    /// Physical inner size, matching what [`DisplayGeometry`] reports.
    #[serde(default = "default_width")]
    pub width: f64,
    #[serde(default = "default_height")]
    pub height: f64,
    #[serde(default)]
    pub fullscreen: bool,
    /// Pin the freshly created window above other windows.
    #[serde(default)]
    pub always_on_top: bool,
}

fn default_width() -> f64 {
    1280.0
}

fn default_height() -> f64 {
    720.0
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

/// Window mode captured when presentation mode was entered, restored on exit.
#[derive(Debug, Clone, Copy)]
struct PresentationRestore {
    fullscreen: bool,
    maximized: bool,
    always_on_top: bool,
}

/// Mutable state shared by the display commands.
#[derive(Default)]
pub struct DisplayRuntime {
    presentation: PresentationOptions,
    /// Stage currently pushed by the session engine, or `None` between sessions.
    session: Option<SessionPresentation>,
    /// Signature of the last broadcast state, used to drop duplicate events.
    last_signature: Option<String>,
    last_geometry_at: Option<Instant>,
    known_monitor_keys: Vec<String>,
    /// True while presentation mode (fullscreen, pinned, chrome-free) is active.
    presentation_mode: bool,
    /// Window mode to go back to when presentation mode is left.
    presentation_restore: Option<PresentationRestore>,
}

/// Managed by Tauri so both webviews and the window event loop see one truth.
#[derive(Default)]
pub struct DisplayRuntimeState(pub Mutex<DisplayRuntime>);

// ---------------------------------------------------------------------------
// Monitor helpers
// ---------------------------------------------------------------------------

/// Monitors are only uniquely identifiable by name plus virtual-desktop origin;
/// the index Windows reports them under is not stable across reconnects.
pub fn monitor_key(monitor: &Monitor) -> String {
    let position = monitor.position();
    format!(
        "{}@{}x{}",
        monitor.name().map(String::as_str).unwrap_or("display"),
        position.x,
        position.y
    )
}

fn descriptor(monitor: &Monitor, primary_key: Option<&str>, control_key: Option<&str>) -> MonitorDescriptor {
    let key = monitor_key(monitor);
    let position = monitor.position();
    let size = monitor.size();

    MonitorDescriptor {
        is_primary: primary_key == Some(key.as_str()),
        is_current: control_key == Some(key.as_str()),
        name: monitor.name().cloned(),
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        scale_factor: monitor.scale_factor(),
        key,
    }
}

/// Enumerates every display Windows reports, marking the primary monitor and
/// the one hosting the control window.
pub fn list_monitors(app: &AppHandle) -> Vec<MonitorDescriptor> {
    let primary = app.primary_monitor().ok().flatten();
    let control = app
        .get_webview_window(CONTROL_WINDOW_LABEL)
        .and_then(|window| window.current_monitor().ok().flatten());

    let primary_key = primary.as_ref().map(monitor_key);
    let control_key = control.as_ref().map(monitor_key);

    let mut monitors = app.available_monitors().unwrap_or_default();
    monitors.sort_by_key(|monitor| (monitor.position().x, monitor.position().y));

    monitors
        .iter()
        .map(|monitor| descriptor(monitor, primary_key.as_deref(), control_key.as_deref()))
        .collect()
}

fn monitor_containing(monitors: &[MonitorDescriptor], x: i32, y: i32) -> Option<&MonitorDescriptor> {
    monitors.iter().find(|monitor| {
        x >= monitor.x
            && y >= monitor.y
            && x < monitor.x + monitor.width as i32
            && y < monitor.y + monitor.height as i32
    })
}

fn centered_in(monitor: &MonitorDescriptor, outer: (u32, u32)) -> (i32, i32) {
    let x = monitor.x + ((monitor.width as i32 - outer.0 as i32) / 2).max(0);
    let y = monitor.y + ((monitor.height as i32 - outer.1 as i32) / 2).max(0);
    (x, y)
}

fn outer_size(window: &WebviewWindow) -> (u32, u32) {
    window
        .outer_size()
        .map(|size| (size.width, size.height))
        .unwrap_or((1280, 720))
}

/// Picks where the window should sit:
/// 1. an explicit remembered position that still falls on a connected display,
/// 2. otherwise the centre of the requested monitor (or the primary one).
fn resolve_position(
    monitors: &[MonitorDescriptor],
    target_key: Option<&str>,
    x: Option<i32>,
    y: Option<i32>,
    window: &WebviewWindow,
) -> Option<PhysicalPosition<i32>> {
    if let (Some(x), Some(y)) = (x, y) {
        if monitor_containing(monitors, x, y).is_some() {
            return Some(PhysicalPosition::new(x, y));
        }
    }

    let target = target_key
        .and_then(|key| monitors.iter().find(|monitor| monitor.key == key))
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| monitors.first())?;

    let (cx, cy) = centered_in(target, outer_size(window));
    Some(PhysicalPosition::new(cx, cy))
}

// ---------------------------------------------------------------------------
// Window state
// ---------------------------------------------------------------------------

fn closed_state() -> DisplayWindowState {
    DisplayWindowState::closed()
}

/// Reads the live state of the presentation window straight from Windows.
pub fn window_state(app: &AppHandle) -> DisplayWindowState {
    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        return closed_state();
    };

    let monitor = window.current_monitor().ok().flatten();
    let presentation = app
        .try_state::<DisplayRuntimeState>()
        .and_then(|runtime| runtime.0.lock().ok().map(|guard| guard.presentation_mode))
        .unwrap_or(false);

    DisplayWindowState {
        open: true,
        fullscreen: window.is_fullscreen().unwrap_or(false),
        maximized: window.is_maximized().unwrap_or(false),
        always_on_top: window.is_always_on_top().unwrap_or(false),
        presentation,
        monitor_key: monitor.as_ref().map(monitor_key),
        monitor_name: monitor.as_ref().and_then(|value| value.name().cloned()),
    }
}

fn signature(state: &DisplayWindowState) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        state.open,
        state.fullscreen,
        state.maximized,
        state.always_on_top,
        state.presentation,
        state.monitor_key.as_deref().unwrap_or("-")
    )
}

/// Broadcasts the coarse window state, skipping payloads that did not change.
pub fn broadcast_state(app: &AppHandle) {
    let state = window_state(app);
    let next = signature(&state);

    {
        let runtime = app.state::<DisplayRuntimeState>();
        let mut guard = match runtime.0.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if guard.last_signature.as_deref() == Some(next.as_str()) {
            return;
        }
        guard.last_signature = Some(next);
    }

    let _ = app.emit(events::DISPLAY_WINDOW_STATE_CHANGED, &state);
}

/// Captures the placement the window should be restored to. The outer position
/// and the *inner* size are captured together with the setters that consume
/// them, so a close/open cycle can never inflate the window by its frame.
fn geometry(app: &AppHandle) -> Option<DisplayGeometry> {
    let window = app.get_webview_window(DISPLAY_WINDOW_LABEL)?;
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    let monitor = window.current_monitor().ok().flatten();

    Some(DisplayGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        monitor_key: monitor.as_ref().map(monitor_key),
    })
}

/// Broadcasts the physical placement of the window so it can be remembered.
pub fn emit_geometry(app: &AppHandle) {
    if let Some(payload) = geometry(app) {
        let _ = app.emit(events::DISPLAY_WINDOW_GEOMETRY, &payload);
    }
}

fn emit_geometry_throttled(app: &AppHandle) {
    {
        let runtime = app.state::<DisplayRuntimeState>();
        let mut guard = match runtime.0.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if let Some(last) = guard.last_geometry_at {
            if last.elapsed() < GEOMETRY_THROTTLE {
                return;
            }
        }
        guard.last_geometry_at = Some(Instant::now());
    }

    emit_geometry(app);
}

// ---------------------------------------------------------------------------
// Window lifecycle
// ---------------------------------------------------------------------------

fn attach_listeners(app: &AppHandle, window: &WebviewWindow) {
    let handle = app.clone();

    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) => {
            broadcast_state(&handle);
            emit_geometry_throttled(&handle);
        }
        WindowEvent::Resized(_) => {
            broadcast_state(&handle);
            emit_geometry_throttled(&handle);
        }
        WindowEvent::ScaleFactorChanged { .. } => {
            broadcast_state(&handle);
            emit_geometry(&handle);
        }
        WindowEvent::CloseRequested { .. } => {
            // Capture the exact resting geometry before the webview is torn down.
            emit_geometry(&handle);
        }
        WindowEvent::Destroyed => {
            broadcast_state(&handle);
        }
        _ => {}
    });
}

/// Creates the presentation window, or brings the existing one forward.
pub fn open_display_window(
    app: &AppHandle,
    options: OpenDisplayOptions,
) -> Result<DisplayWindowState, String> {
    let monitors = list_monitors(app);

    if let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();

        if options.fullscreen && !window.is_fullscreen().unwrap_or(false) {
            let _ = window.set_fullscreen(true);
        }

        if options.always_on_top != window.is_always_on_top().unwrap_or(false) {
            let _ = window.set_always_on_top(options.always_on_top);
        }

        if let Some(position) = resolve_position(
            &monitors,
            options.monitor_key.as_deref(),
            options.x,
            options.y,
            &window,
        ) {
            let _ = window.set_position(position);
        }

        let _ = window.set_focus();
        broadcast_state(app);
        emit_geometry(app);
        return Ok(window_state(app));
    }

    let window = WebviewWindowBuilder::new(
        app,
        DISPLAY_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("app.timer — Display")
    .inner_size(options.width.max(MIN_WIDTH), options.height.max(MIN_HEIGHT))
    .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
    .resizable(true)
    .decorations(true)
    .always_on_top(options.always_on_top)
    .visible(false)
    .build()
    .map_err(|error| format!("Could not create the display window: {error}"))?;

    attach_listeners(app, &window);

    // The remembered geometry is reported in physical pixels, so it is applied
    // through the physical setter to stay DPI-independent.
    let _ = window.set_size(PhysicalSize::new(
        options.width.max(MIN_WIDTH) as u32,
        options.height.max(MIN_HEIGHT) as u32,
    ));

    if let Some(position) = resolve_position(
        &monitors,
        options.monitor_key.as_deref(),
        options.x,
        options.y,
        &window,
    ) {
        let _ = window.set_position(position);
    }

    // Never force fullscreen on open: the caller opts in explicitly.
    if options.fullscreen {
        let _ = window.set_fullscreen(true);
    }

    let _ = window.show();
    let _ = window.set_focus();

    broadcast_state(app);
    emit_geometry(app);
    Ok(window_state(app))
}

/// Closes the presentation window. The control window stays untouched.
pub fn close_display_window(app: &AppHandle) -> Result<DisplayWindowState, String> {
    // Presentation mode does not outlive the window it was applied to, so a
    // reopen never inherits a chrome-free, pinned frame it did not ask for.
    {
        let runtime = app.state::<DisplayRuntimeState>();
        if let Ok(mut guard) = runtime.0.lock() {
            guard.presentation_mode = false;
            guard.presentation_restore = None;
        }
    }

    if let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) {
        emit_geometry(app);
        window
            .close()
            .map_err(|error| format!("Could not close the display window: {error}"))?;
    }

    Ok(window_state(app))
}

/// Switches the presentation window between windowed and fullscreen modes.
pub fn set_display_fullscreen(
    app: &AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        return Err("The display window is not open.".to_string());
    };

    window
        .set_fullscreen(enabled)
        .map_err(|error| format!("Could not change fullscreen: {error}"))?;

    if !enabled {
        // Leaving fullscreen also leaves presentation mode: the mode is defined
        // by owning the whole screen, so the remaining half of it is undone too.
        {
            let runtime = app.state::<DisplayRuntimeState>();
            if let Ok(mut guard) = runtime.0.lock() {
                guard.presentation_mode = false;
                guard.presentation_restore = None;
            }
        }
        let _ = window.set_decorations(true);
        let _ = window.set_always_on_top(false);
        let _ = window.unminimize();
        let _ = window.show();
    }

    let _ = window.set_focus();
    broadcast_state(app);
    emit_geometry(app);
    Ok(window_state(app))
}

/// Returns the window to a visible, usable windowed state.
pub fn restore_display_window(app: &AppHandle) -> Result<DisplayWindowState, String> {
    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        return Err("The display window is not open.".to_string());
    };

    // Restoring is an explicit return to the ordinary windowed presentation, so
    // presentation mode is left behind rather than left half-applied.
    {
        let runtime = app.state::<DisplayRuntimeState>();
        if let Ok(mut guard) = runtime.0.lock() {
            guard.presentation_mode = false;
            guard.presentation_restore = None;
        }
    }

    let _ = window.set_decorations(true);
    let _ = window.set_fullscreen(false);
    let _ = window.unmaximize();
    let _ = window.unminimize();
    let _ = window.show();

    let monitors = list_monitors(app);
    let position = window
        .outer_position()
        .unwrap_or(PhysicalPosition::new(0, 0));

    // A window left behind on a disconnected monitor is brought back.
    if monitor_containing(&monitors, position.x, position.y).is_none() {
        if let Some(target) = monitors
            .iter()
            .find(|monitor| monitor.is_primary)
            .or_else(|| monitors.first())
        {
            let (x, y) = centered_in(target, outer_size(&window));
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }

    let _ = window.set_focus();
    broadcast_state(app);
    emit_geometry(app);
    Ok(window_state(app))
}

/// Pins or unpins the presentation window above other windows.
pub fn set_display_always_on_top(
    app: &AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        return Err("The display window is not open.".to_string());
    };

    window
        .set_always_on_top(enabled)
        .map_err(|error| format!("Could not change always-on-top: {error}"))?;

    broadcast_state(app);
    Ok(window_state(app))
}

/// Maximises the presentation window, or returns it to its restored size.
///
/// Windows ignores `maximize` while the window owns the whole screen, so
/// fullscreen is left first — the operator asked for a maximised window, not a
/// fullscreen one.
pub fn set_display_maximized(
    app: &AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        return Err("The display window is not open.".to_string());
    };

    if window.is_fullscreen().unwrap_or(false) {
        let _ = window.set_fullscreen(false);
    }

    if enabled {
        let _ = window.maximize();
    } else {
        let _ = window.unmaximize();
    }

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    broadcast_state(app);
    emit_geometry(app);
    Ok(window_state(app))
}

/// Enters or leaves presentation mode.
///
/// Presentation mode combines fullscreen, always-on-top and a chrome-free
/// frame. The window mode that was active when it was entered is remembered and
/// restored on the way out, so leaving can never strand the window in a state
/// the operator did not choose. This remains a mode of the existing window: no
/// second native window is ever created for it.
pub fn set_display_presentation(
    app: &AppHandle,
    enabled: bool,
) -> Result<DisplayWindowState, String> {
    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        return Err("The display window is not open.".to_string());
    };

    if enabled {
        let restore = PresentationRestore {
            fullscreen: window.is_fullscreen().unwrap_or(false),
            maximized: window.is_maximized().unwrap_or(false),
            always_on_top: window.is_always_on_top().unwrap_or(false),
        };
        {
            let runtime = app.state::<DisplayRuntimeState>();
            if let Ok(mut guard) = runtime.0.lock() {
                // Re-entering an already active presentation mode keeps the
                // original restore point rather than overwriting it with the
                // mode presentation mode itself applied.
                if !guard.presentation_mode {
                    guard.presentation_restore = Some(restore);
                }
                guard.presentation_mode = true;
            }
        }

        let _ = window.set_decorations(false);
        let _ = window.unmaximize();
        let _ = window.set_fullscreen(true);
        let _ = window.set_always_on_top(true);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        let restore = {
            let runtime = app.state::<DisplayRuntimeState>();
            match runtime.0.lock() {
                Ok(mut guard) => {
                    guard.presentation_mode = false;
                    guard.presentation_restore.take()
                }
                Err(_) => None,
            }
        };

        let _ = window.set_decorations(true);
        let _ = window.set_fullscreen(restore.is_some_and(|value| value.fullscreen));
        let _ = window.set_always_on_top(restore.is_some_and(|value| value.always_on_top));

        if restore.is_some_and(|value| value.maximized) {
            let _ = window.maximize();
        } else {
            let _ = window.unmaximize();
        }

        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    broadcast_state(app);
    emit_geometry(app);
    Ok(window_state(app))
}

/// Moves (or opens) the presentation window on the requested monitor.
pub fn move_display_to_monitor(
    app: &AppHandle,
    monitor_key: &str,
) -> Result<DisplayWindowState, String> {
    let monitors = list_monitors(app);

    if !monitors.iter().any(|monitor| monitor.key == monitor_key) {
        return Err("That monitor is no longer connected.".to_string());
    }

    let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) else {
        let options = OpenDisplayOptions {
            monitor_key: Some(monitor_key.to_string()),
            x: None,
            y: None,
            width: default_width(),
            height: default_height(),
            fullscreen: false,
            always_on_top: false,
        };
        return open_display_window(app, options);
    };

    // Windows ignores manual positioning while a window is fullscreen.
    if window.is_fullscreen().unwrap_or(false) {
        let _ = window.set_fullscreen(false);
    }
    let _ = window.unmaximize();
    let _ = window.unminimize();
    let _ = window.show();

    if let Some(target) = monitors.iter().find(|monitor| monitor.key == monitor_key) {
        let (x, y) = centered_in(target, outer_size(&window));
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }

    let _ = window.set_focus();
    broadcast_state(app);
    emit_geometry(app);
    Ok(window_state(app))
}

/// Re-reads the connected displays. Emits a change event when the set differs
/// from the last check and re-homes a window stranded on a removed monitor.
pub fn sync_display(app: &AppHandle) -> Vec<MonitorDescriptor> {
    let monitors = list_monitors(app);
    let keys: Vec<String> = monitors.iter().map(|monitor| monitor.key.clone()).collect();

    let monitors_changed = {
        let runtime = app.state::<DisplayRuntimeState>();
        match runtime.0.lock() {
            Ok(mut guard) => {
                let changed = guard.known_monitor_keys != keys;
                guard.known_monitor_keys = keys;
                changed
            }
            Err(_) => false,
        }
    };

    if monitors_changed {
        let _ = app.emit(events::MONITORS_CHANGED, &monitors);
    }

    if let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) {
        let position = window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));

        if monitor_containing(&monitors, position.x, position.y).is_none() {
            let _ = window.set_fullscreen(false);
            let _ = window.unmaximize();

            if let Some(target) = monitors
                .iter()
                .find(|monitor| monitor.is_primary)
                .or_else(|| monitors.first())
            {
                let (x, y) = centered_in(target, outer_size(&window));
                let _ = window.set_position(PhysicalPosition::new(x, y));
            }
        }

        broadcast_state(app);
        emit_geometry(app);
    }

    monitors
}

// ---------------------------------------------------------------------------
// Shared presentation state
// ---------------------------------------------------------------------------

/// Stores the presentation options and broadcasts them to every webview.
pub fn set_presentation_options(
    app: &AppHandle,
    options: PresentationOptions,
) -> PresentationOptions {
    {
        let runtime = app.state::<DisplayRuntimeState>();
        if let Ok(mut guard) = runtime.0.lock() {
            guard.presentation = options.clone();
        }
    }

    let _ = app.emit(events::DISPLAY_PRESENTATION_CHANGED, &options);
    options
}

/// Returns the last options pushed by the control window. The display window
/// calls this on startup so it recovers even if it missed earlier broadcasts.
pub fn presentation_options(app: &AppHandle) -> PresentationOptions {
    let runtime = app.state::<DisplayRuntimeState>();
    match runtime.0.lock() {
        Ok(guard) => guard.presentation.clone(),
        Err(_) => PresentationOptions::default(),
    }
}

/// Stores and broadcasts the stage the session engine is currently on.
///
/// `None` clears it — the display window falls back to the ordinary timer
/// presentation once the session ends. Passed `None` it never touches the
/// timer engine's own state, so ordinary single-timer operation is unaffected.
pub fn sync_session(app: &AppHandle, presentation: Option<SessionPresentation>) {
    {
        let runtime = app.state::<DisplayRuntimeState>();
        if let Ok(mut guard) = runtime.0.lock() {
            guard.session = presentation.clone();
        }
    }

    let _ = app.emit(events::DISPLAY_SESSION_CHANGED, &presentation);
}

/// Returns the stage last pushed by the session engine, if any. The display
/// window calls this on startup so it recovers after a restart.
pub fn session_presentation(app: &AppHandle) -> Option<SessionPresentation> {
    let runtime = app.state::<DisplayRuntimeState>();
    match runtime.0.lock() {
        Ok(guard) => guard.session.clone(),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(
        always_on_top: bool,
        presentation: bool,
        monitor_key: Option<&str>,
        monitor_name: Option<&str>,
    ) -> DisplayWindowState {
        DisplayWindowState {
            open: true,
            fullscreen: presentation,
            maximized: false,
            always_on_top,
            presentation,
            monitor_key: monitor_key.map(str::to_string),
            monitor_name: monitor_name.map(str::to_string),
        }
    }

    #[test]
    fn signature_carries_every_field_the_control_window_reacts_to() {
        let base = window(false, false, Some("screen@0x0"), Some("Screen 1"));

        // Pinning, entering presentation mode and moving to another monitor are
        // all observable, so none of them may be swallowed by the filter that
        // drops duplicate window-state broadcasts.
        assert_ne!(
            signature(&base),
            signature(&window(true, false, Some("screen@0x0"), Some("Screen 1")))
        );
        assert_ne!(
            signature(&base),
            signature(&window(false, true, Some("screen@0x0"), Some("Screen 1")))
        );
        assert_ne!(
            signature(&base),
            signature(&window(false, false, Some("screen@1920x0"), Some("Screen 1")))
        );
    }

    #[test]
    fn signature_keys_off_the_stable_monitor_identity() {
        // A monitor's friendly name is not what identifies it — the key, which
        // also carries its virtual-desktop origin, is.
        let a = window(false, false, Some("screen@0x0"), Some("Screen 1"));
        let b = window(false, false, Some("screen@0x0"), Some("Renamed display"));
        assert_eq!(signature(&a), signature(&b));
    }

    #[test]
    fn the_style_document_uses_the_keys_the_frontend_reads() {
        let options = PresentationOptions::default();
        let json = serde_json::to_value(&options).expect("options serialise");

        assert!(json.get("timerFormat").is_some());
        assert!(json["style"]["layout"].get("showProgressBar").is_some());
        assert!(json["style"]["layout"].get("showStageInfo").is_some());
        assert!(json["style"]["layout"].get("overtimeCorner").is_some());
        assert!(json["style"]["theme"].get("backgroundImage").is_some());
        assert!(json["style"].get("messageText").is_some());

        // A document round-tripped through the bridge is unchanged.
        let restored: PresentationOptions = serde_json::from_value(json).expect("options load");
        assert_eq!(restored.style.timer.font, options.style.timer.font);
        assert_eq!(restored.style.timer.weight, options.style.timer.weight);
        assert_eq!(restored.style.message_text.font, options.style.message_text.font);
        assert_eq!(restored.style.layout.spacing, options.style.layout.spacing);
        assert_eq!(restored.style.theme.progress_color, options.style.theme.progress_color);
    }

    #[test]
    fn a_payload_written_before_the_style_existed_still_loads() {
        // An earlier control window only pushed the timer format; the style has
        // to fall back to its defaults rather than failing to deserialise.
        let options: PresentationOptions =
            serde_json::from_str(r#"{"timerFormat":"mm:ss"}"#).expect("legacy payload loads");

        assert_eq!(options.timer_format, "mm:ss");
        assert!(options.message.is_none());
        assert_eq!(options.style.layout.timer_position, "center");
        assert_eq!(options.style.timer.font, "mono");
        assert_eq!(options.style.message_text.font, "sans");
    }

    #[test]
    fn a_message_without_a_display_duration_is_sticky() {
        let message: DisplayMessage = serde_json::from_str(
            r##"{"id":"m1","name":"Note","text":"Hello","fontSize":104,"color":"#FFFFFF","backgroundColor":null,"position":"bottom"}"##,
        )
        .expect("message loads");

        assert_eq!(message.duration_ms, 0);
        assert_eq!(message.animation, "");
    }

    #[test]
    fn a_message_display_duration_survives_the_bridge() {
        let message: DisplayMessage = serde_json::from_str(
            r##"{"id":"m1","name":"Note","text":"Hello","fontSize":104,"color":"#FFFFFF","backgroundColor":null,"position":"bottom","animation":"zoom","durationMs":8000}"##,
        )
        .expect("message loads");

        assert_eq!(message.duration_ms, 8000);
        assert_eq!(message.animation, "zoom");
    }
}
