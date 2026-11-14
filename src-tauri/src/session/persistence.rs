//! Session persistence.
//!
//! The session *cursor* — which stage is current, whether the session is
//! waiting and any transition deadline — has to survive a restart. The stage
//! countdown does not: each stage is an ordinary timer, so the timer engine
//! already persists the running stage's remaining time. Together they are
//! enough to resume a session exactly where it stopped.
//!
//! Like the timer document, every operation degrades to "no session" instead
//! of failing: a missing, unreadable or corrupted file must never stop the
//! application from starting.

use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::state::SessionRecord;

/// File the session cursor is written to, inside the app data directory.
pub const SESSION_STATE_FILE: &str = "session-state.json";

/// Absolute path of the state file, creating the app data directory if needed.
pub fn resolve_path(app: &AppHandle) -> Option<PathBuf> {
    let directory = app.path().app_data_dir().ok()?;
    if fs::create_dir_all(&directory).is_err() {
        return None;
    }
    Some(directory.join(SESSION_STATE_FILE))
}

/// Reads the persisted cursor, or `None` when there is nothing usable.
pub fn load(path: &Path) -> Option<SessionRecord> {
    let raw = fs::read_to_string(path).ok()?;
    let record: SessionRecord = serde_json::from_str(&raw).ok()?;
    if record.definition.stages.is_empty() || record.definition.id.is_empty() {
        return None;
    }
    Some(record)
}

/// Writes the cursor atomically, replacing the previous document.
pub fn save(path: &Path, record: &SessionRecord) {
    let Ok(payload) = serde_json::to_string_pretty(record) else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    if fs::write(&temporary, payload).is_err() {
        return;
    }
    let _ = fs::rename(&temporary, path);
}

/// Removes the document, so a cleared session leaves nothing to recover.
pub fn clear(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::state::{SessionDefinition, SessionStage, SessionStatus, StageTransition};

    fn record() -> SessionRecord {
        SessionRecord {
            definition: SessionDefinition {
                id: "session-1".to_string(),
                name: "Examination".to_string(),
                stages: vec![SessionStage {
                    id: "stage-1".to_string(),
                    name: "Reading".to_string(),
                    duration_ms: 60_000,
                    message: None,
                    warning_intervals_ms: vec![10_000],
                    sound_enabled: true,
                    transition: StageTransition::Manual,
                    transition_countdown_ms: None,
                }],
            },
            stage_index: 0,
            status: SessionStatus::Awaiting,
            awaiting_manual: true,
            transition_deadline_at: None,
            fired_warnings_ms: vec![10_000],
            updated_at: 1_700_000_000_000,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("app-timer-session-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the temporary directory is writable");
        path.join(SESSION_STATE_FILE)
    }

    #[test]
    fn a_saved_cursor_round_trips() {
        let path = scratch("round-trip");
        save(&path, &record());

        let loaded = load(&path).expect("the cursor was just written");
        assert_eq!(loaded.definition.id, "session-1");
        assert_eq!(loaded.stage_index, 0);
        assert_eq!(loaded.status, SessionStatus::Awaiting);
        assert!(loaded.awaiting_manual);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupted_json_is_reported_as_absent() {
        let path = scratch("corrupted");
        fs::write(&path, "{ not json").expect("the scratch file is writable");
        assert!(load(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_document_without_stages_is_discarded() {
        let path = scratch("empty");
        let mut empty = record();
        empty.definition.stages.clear();
        save(&path, &empty);
        assert!(load(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clearing_removes_the_document() {
        let path = scratch("clear");
        save(&path, &record());
        clear(&path);
        assert!(load(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
