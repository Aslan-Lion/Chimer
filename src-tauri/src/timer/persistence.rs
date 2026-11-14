//! Timer persistence.
//!
//! Milestone 3 needs the *runtime* timer state (config, state, remaining time
//! and timestamps) to survive a restart. The project's SQLite layer documented
//! in [`crate::database`] does not exist yet — that database is scheduled for
//! Milestone 5 and will own the library, presets, messages and history — so the
//! timer runtime state is written to a single small JSON document in the
//! platform application-data directory. Swapping this module for SQLite later
//! only has to preserve the same [`TimerRecord`] shape.
//!
//! Every operation degrades to "no persisted state" instead of failing: a
//! missing, unreadable or corrupted file must never stop the timer engine from
//! running, it must only mean the application starts with no timer.

use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::state::TimerRecord;

/// File the timer runtime state is written to, inside the app data directory.
pub const TIMER_STATE_FILE: &str = "timer-state.json";

/// Absolute path of the state file, creating the app data directory if needed.
///
/// Returns `None` when the directory cannot be resolved or created, in which
/// case the engine keeps working in memory only.
pub fn resolve_path(app: &AppHandle) -> Option<PathBuf> {
    let directory = app.path().app_data_dir().ok()?;
    if fs::create_dir_all(&directory).is_err() {
        return None;
    }
    Some(directory.join(TIMER_STATE_FILE))
}

/// Reads the persisted record.
///
/// A record that cannot be parsed, or that cannot describe a usable timer, is
/// reported as absent so a half-written file is treated as "no timer" rather
/// than crashing the startup path.
pub fn load(path: &Path) -> Option<TimerRecord> {
    let raw = fs::read_to_string(path).ok()?;
    let record: TimerRecord = serde_json::from_str(&raw).ok()?;
    if record.timer_id.is_none() || record.duration_ms <= 0 {
        return None;
    }
    Some(record)
}

/// Writes the record, replacing the previous document atomically.
///
/// The payload is written to a temporary file first and then renamed over the
/// target, so an interrupted write can never leave a truncated state file
/// behind.
pub fn save(path: &Path, record: &TimerRecord) {
    let Ok(payload) = serde_json::to_string_pretty(record) else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    if fs::write(&temporary, payload).is_err() {
        return;
    }
    let _ = fs::rename(&temporary, path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::state::TimerStatus;

    fn record() -> TimerRecord {
        TimerRecord {
            timer_id: Some("timer-1".to_string()),
            name: "Examination".to_string(),
            duration_ms: 90 * 60 * 1_000,
            remaining_ms: 12_000,
            status: TimerStatus::Paused,
            revision: 7,
            updated_at: 1_700_000_000_000,
            started_at: Some(1_699_999_000_000),
            completed_at: None,
            deadline_at: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("app-timer-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the temporary directory is writable");
        path.join(TIMER_STATE_FILE)
    }

    #[test]
    fn a_saved_record_round_trips() {
        let path = scratch("round-trip");
        save(&path, &record());

        let loaded = load(&path).expect("the record was just written");
        assert_eq!(loaded.timer_id.as_deref(), Some("timer-1"));
        assert_eq!(loaded.name, "Examination");
        assert_eq!(loaded.remaining_ms, 12_000);
        assert_eq!(loaded.status, TimerStatus::Paused);
        assert_eq!(loaded.revision, 7);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn saving_replaces_the_previous_document_without_leaving_a_partial_file() {
        let path = scratch("replace");
        save(&path, &record());

        let mut updated = record();
        updated.revision = 8;
        updated.status = TimerStatus::Running;
        updated.deadline_at = Some(1_700_000_600_000);
        save(&path, &updated);

        let loaded = load(&path).expect("the second write is readable");
        assert_eq!(loaded.revision, 8);
        assert_eq!(loaded.status, TimerStatus::Running);
        assert_eq!(loaded.deadline_at, Some(1_700_000_600_000));

        // The scratch file used for the atomic replace must not survive.
        assert!(!path.with_extension("json.tmp").exists());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_file_is_reported_as_absent() {
        let path = scratch("missing");
        assert!(load(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupted_json_is_reported_as_absent() {
        let path = scratch("corrupt");
        fs::write(&path, "{ this is not json").unwrap();
        assert!(load(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_record_that_cannot_describe_a_timer_is_reported_as_absent() {
        let path = scratch("unusable");

        let mut without_id = record();
        without_id.timer_id = None;
        fs::write(&path, serde_json::to_string(&without_id).unwrap()).unwrap();
        assert!(load(&path).is_none());

        let mut without_duration = record();
        without_duration.duration_ms = 0;
        fs::write(&path, serde_json::to_string(&without_duration).unwrap()).unwrap();
        assert!(load(&path).is_none());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
