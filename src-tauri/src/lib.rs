//! app.timer — Tauri backend entry point.
//!
//! The module layout mirrors the development milestones so that timer logic,
//! native window management, persistence and the IPC surface never collapse
//! into a single file:
//!
//! * [`commands`] — the IPC surface shared by both webviews
//! * [`events`]   — event channel names used to synchronise windows
//! * [`timer`]    — authoritative countdown engine (Milestone 3)
//! * [`session`]  — sequential stage execution over that same engine (Milestone 4)
//! * [`display`]  — native presentation window management and the shared
//!   presentation state: window modes, style, message and current stage
//!   (Milestones 2 and 5)
//! * [`network`]  — the local WebSocket host remote controllers and remote
//!   displays connect to (Milestone 6)
//! * [`database`] — reserved for the history and analytics store; the running
//!   timer and session documents are persisted by their own engines

mod commands;
mod database;
mod display;
mod events;
mod network;
mod session;
mod timer;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{Manager, WindowEvent};

/// How often the connected display set is re-read. Windows exposes no
/// monitor-change callback through the Tauri window API, so disconnect and
/// reconnect are noticed by polling; the check is cheap and only emits when
/// the set actually differs.
const MONITOR_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// How often the timer engine is asked whether a deadline has arrived.
///
/// This is *not* how the countdown is kept: the remaining time is recalculated
/// from the timer's monotonic anchor on every read. The heartbeat only exists so
/// a deadline is noticed and broadcast promptly, and so both windows refresh
/// smoothly while a timer is running or accruing overtime.
const TIMER_TICK_INTERVAL: Duration = Duration::from_millis(250);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(display::DisplayRuntimeState::default())
        .manage(timer::TimerEngine::new())
        .manage(session::engine::SessionEngine::new())
        .manage(Arc::new(network::NetworkEngine::new()))
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_monitors,
            commands::sync_display,
            commands::display_window_state,
            commands::open_display_window,
            commands::close_display_window,
            commands::set_display_fullscreen,
            commands::restore_display_window,
            commands::set_display_always_on_top,
            commands::set_display_maximized,
            commands::set_display_presentation,
            commands::move_display_to_monitor,
            commands::set_presentation_options,
            commands::presentation_options,
            commands::session_presentation,
            timer::commands::timer_snapshot,
            timer::commands::timer_create,
            timer::commands::timer_start,
            timer::commands::timer_pause,
            timer::commands::timer_resume,
            timer::commands::timer_reset,
            timer::commands::timer_abort,
            timer::commands::timer_stop,
            session::commands::session_snapshot,
            session::commands::session_start,
            session::commands::session_next,
            session::commands::session_stop,
            network::commands::network_status,
            network::commands::network_refresh,
            network::commands::network_start,
            network::commands::network_stop,
            network::commands::network_addresses,
            network::commands::network_issue_pairing,
            network::commands::network_pending_requests,
            network::commands::network_approve_pairing,
            network::commands::network_reject_pairing,
            network::commands::network_devices,
            network::commands::network_connections,
            network::commands::network_set_role,
            network::commands::network_set_permissions,
            network::commands::network_revoke_device,
            network::commands::network_disconnect,
            network::commands::network_set_library,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Tear the presentation window down with the control window so the
            // process never lingers as an orphaned second window.
            if let Some(control) = app.get_webview_window(display::CONTROL_WINDOW_LABEL) {
                let close_handle = handle.clone();
                control.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Destroyed) {
                        // Close the listener and every remote socket before the
                        // process goes away, so a paired device learns at once
                        // that the host is gone rather than waiting on a timeout.
                        if let Some(network) = network::engine::engine_of(&close_handle) {
                            network.stop(&close_handle);
                        }
                        let _ = display::close_display_window(&close_handle);
                    }
                });
            }

            // Restore the persisted timer before any window can query it, so the
            // first snapshot a window sees is already the recovered one.
            app.state::<timer::TimerEngine>().initialize(&handle);

            // The session cursor is restored straight after: it only records
            // *which* stage the restored timer belongs to, so the running
            // stage's remaining time is never re-armed here.
            app.state::<session::engine::SessionEngine>().initialize(&handle);

            // Local networking starts disabled: the paired-device register is
            // restored and the machine's addresses are read, but no listener is
            // bound until the operator turns it on in Settings. A host that
            // never enables networking behaves exactly as it did in Milestone 5.
            if let Some(engine) = network::engine::engine_of(&handle) {
                engine.initialize(&handle);
            }

            display::sync_display(&handle);

            let poll_handle = handle.clone();
            thread::spawn(move || loop {
                thread::sleep(MONITOR_POLL_INTERVAL);
                display::sync_display(&poll_handle);
            });

            let tick_handle = handle.clone();
            thread::spawn(move || loop {
                thread::sleep(TIMER_TICK_INTERVAL);
                // The timer settles first, so a stage that reached zero is
                // already reported as completed when the session inspects it.
                if let Some(engine) = tick_handle.try_state::<timer::TimerEngine>() {
                    engine.tick(&tick_handle);
                }
                if let Some(session) = tick_handle.try_state::<session::engine::SessionEngine>() {
                    session.tick(&tick_handle);
                }
                // The network heartbeat expires pairing state and republishes
                // the authoritative snapshot to every connected remote.
                if let Some(network) = network::engine::engine_of(&tick_handle) {
                    network.tick(&tick_handle);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running app.timer");
}
