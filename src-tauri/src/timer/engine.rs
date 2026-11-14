//! The authoritative timer engine.
//!
//! One [`TimerEngine`] instance is created per process and managed by Tauri, so
//! every window talks to the same state. It owns three responsibilities:
//!
//! * run the state machine in [`super::state`] under a lock,
//! * write the resulting state through to [`super::persistence`],
//! * broadcast the resulting snapshot to both windows through [`super::events`].
//!
//! # Clock handling
//!
//! In-process elapsed time is measured with the monotonic [`Instant`] clock, so
//! the countdown cannot drift and is immune to system-clock changes while the
//! application is running. The wall clock is only used to stamp
//! `startedAt` / `completedAt` and to persist a recoverable deadline for a
//! running timer. While the application is *closed*, that wall-clock deadline is
//! the recovery anchor: time that passes is treated as elapsed, and a system
//! clock change during that window is therefore interpreted as elapsed or
//! remaining time and clamped into the configured duration.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tauri::AppHandle;

use super::events;
use super::persistence;
use super::state::{TimerMachine, TimerSnapshot, TimerStatus};

/// Milliseconds since the Unix epoch — the only clock shared with the webviews.
pub fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Authoritative owner of the single active timer.
pub struct TimerEngine {
    machine: Mutex<TimerMachine>,
    /// Where state changes are written. `None` until the engine is initialised
    /// (or when the application-data directory is unavailable), in which case
    /// the engine still runs, it just cannot be recovered after a restart.
    store: Mutex<Option<PathBuf>>,
}

impl Default for TimerEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerEngine {
    pub fn new() -> Self {
        Self {
            machine: Mutex::new(TimerMachine::empty(wall_now_ms())),
            store: Mutex::new(None),
        }
    }

    /// Restores the persisted timer and remembers where future changes go.
    ///
    /// Called once during startup, before any window can issue a timer command.
    /// The restored snapshot is broadcast immediately so a window that is
    /// already listening does not have to ask for it.
    pub fn initialize(&self, app: &AppHandle) {
        let Some(path) = persistence::resolve_path(app) else {
            return;
        };

        if let Some(record) = persistence::load(&path) {
            let now = Instant::now();
            let restored = TimerMachine::restore(record, now, wall_now_ms());
            *self.machine() = restored;
        }

        *self.store() = Some(path);

        let snapshot = self.current();
        events::emit_state(app, &snapshot);
    }

    // ------------------------------------------------------------- commands

    /// Configures a new timer and returns it to `Idle`.
    pub fn create(
        &self,
        app: &AppHandle,
        name: String,
        duration_ms: i64,
    ) -> Result<TimerSnapshot, String> {
        self.apply(app, move |machine, _now, wall| {
            machine.create(name, duration_ms, wall)
        })
    }

    /// Starts the countdown from the full configured duration.
    pub fn start(&self, app: &AppHandle) -> Result<TimerSnapshot, String> {
        self.apply(app, |machine, now, wall| machine.start(now, wall))
    }

    /// Freezes the countdown, preserving the exact remaining time.
    pub fn pause(&self, app: &AppHandle) -> Result<TimerSnapshot, String> {
        self.apply(app, |machine, now, wall| machine.pause(now, wall))
    }

    /// Continues a paused countdown from a fresh anchor.
    pub fn resume(&self, app: &AppHandle) -> Result<TimerSnapshot, String> {
        self.apply(app, |machine, now, wall| machine.resume(now, wall))
    }

    /// Restores the original duration and returns the timer to `Idle`.
    pub fn reset(&self, app: &AppHandle) -> Result<TimerSnapshot, String> {
        self.apply(app, |machine, _now, wall| machine.reset(wall))
    }

    /// Aborts the timer outright, discarding it and returning the engine to the
    /// empty `Idle` state.
    ///
    /// Both windows fall back to the idle presentation (`00:00`, zero progress,
    /// no overtime) and the cleared state is written through, so a restart cannot
    /// resurrect the aborted timer.
    pub fn abort(&self, app: &AppHandle) -> Result<TimerSnapshot, String> {
        self.apply(app, |machine, _now, wall| {
            machine.abort(wall);
            Ok(())
        })
    }

    /// Terminates the countdown, preserving the remaining time.
    pub fn stop(&self, app: &AppHandle) -> Result<TimerSnapshot, String> {
        self.apply(app, |machine, now, wall| machine.stop(now, wall))
    }

    /// Current authoritative snapshot, without broadcasting.
    ///
    /// Used to answer a window that has explicitly asked for the latest state —
    /// most importantly the display window when it opens.
    pub fn current(&self) -> TimerSnapshot {
        self.refresh().0
    }

    /// Background heartbeat.
    ///
    /// Detects a countdown that reached its deadline and keeps both windows
    /// updated while the timer is running or accumulating overtime. Nothing is
    /// broadcast while the timer is idle, paused or stopped, because then the
    /// snapshot cannot change.
    pub fn tick(&self, app: &AppHandle) {
        let (snapshot, completed) = self.refresh();

        let live = matches!(
            snapshot.status,
            TimerStatus::Running | TimerStatus::Completed
        );
        if completed || live {
            events::emit_state(app, &snapshot);
            if completed {
                events::emit_completed(app, &snapshot);
            }
        }
    }

    // ------------------------------------------------------------ internals

    /// Runs one state transition, writes the result through to disk and
    /// broadcasts it.
    fn apply<F>(&self, app: &AppHandle, action: F) -> Result<TimerSnapshot, String>
    where
        F: FnOnce(&mut TimerMachine, Instant, i64) -> Result<(), String>,
    {
        let (snapshot, completed) = self.transact(action)?;
        events::emit_state(app, &snapshot);
        if completed {
            events::emit_completed(app, &snapshot);
        }
        Ok(snapshot)
    }

    /// Applies `action` under the lock and persists the outcome.
    ///
    /// A countdown that reached its deadline is settled first, so a command that
    /// arrives in the same tick as the deadline can never resurrect an expired
    /// timer. The second element of the result reports whether that settlement
    /// happened, so completion is announced exactly once.
    fn transact<F>(&self, action: F) -> Result<(TimerSnapshot, bool), String>
    where
        F: FnOnce(&mut TimerMachine, Instant, i64) -> Result<(), String>,
    {
        let now = Instant::now();
        let wall = wall_now_ms();

        let (outcome, completed, snapshot, record) = {
            let mut machine = self.machine();
            let completed = machine.settle(now, wall);
            let outcome = action(&mut machine, now, wall);
            (
                outcome,
                completed,
                machine.snapshot(now, wall),
                machine.to_record(now),
            )
        };

        self.persist(&record);
        outcome?;
        Ok((snapshot, completed))
    }

    /// Settles a due completion and returns the fresh snapshot.
    ///
    /// The document is rewritten only when a completion actually happened, so
    /// ticking never turns into per-interval file I/O.
    fn refresh(&self) -> (TimerSnapshot, bool) {
        let now = Instant::now();
        let wall = wall_now_ms();

        let (completed, snapshot, record) = {
            let mut machine = self.machine();
            let completed = machine.settle(now, wall);
            (
                completed,
                machine.snapshot(now, wall),
                machine.to_record(now),
            )
        };

        if completed {
            self.persist(&record);
        }

        (snapshot, completed)
    }

    fn persist(&self, record: &super::state::TimerRecord) {
        let path = self.store().clone();
        if let Some(path) = path {
            persistence::save(&path, record);
        }
    }

    /// Locking helpers that survive a poisoned mutex instead of turning one
    /// failed operation into an application-wide failure.
    fn machine(&self) -> MutexGuard<'_, TimerMachine> {
        self.machine.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn store(&self) -> MutexGuard<'_, Option<PathBuf>> {
        self.store.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    const SECOND: i64 = 1_000;

    /// An engine wired to a scratch file, which is all `transact` needs. No Tauri
    /// application is involved, so the write-through behaviour is testable.
    fn engine_at(path: PathBuf) -> TimerEngine {
        TimerEngine {
            machine: Mutex::new(TimerMachine::empty(wall_now_ms())),
            store: Mutex::new(Some(path)),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let mut directory = std::env::temp_dir();
        directory.push(format!("app-timer-engine-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("the temporary directory is writable");
        directory.join("timer-state.json")
    }

    #[test]
    fn state_changes_are_written_through_to_disk() {
        let path = scratch("write-through");
        let engine = engine_at(path.clone());

        let created = engine
            .transact(|machine, _now, wall| machine.create("Exam".to_string(), 90 * SECOND, wall))
            .expect("creating a valid timer succeeds")
            .0;
        assert_eq!(created.status, TimerStatus::Idle);

        let persisted = persistence::load(&path).expect("the created timer was persisted");
        assert_eq!(persisted.name, "Exam");
        assert_eq!(persisted.status, TimerStatus::Idle);
        assert_eq!(persisted.remaining_ms, 90 * SECOND);

        engine
            .transact(|machine, now, wall| machine.start(now, wall))
            .expect("starting an idle timer succeeds");

        let persisted = persistence::load(&path).expect("the running timer was persisted");
        assert_eq!(persisted.status, TimerStatus::Running);
        // A running timer is recovered from its deadline rather than a counter.
        assert!(persisted.deadline_at.is_some());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_rejected_command_leaves_the_persisted_state_unchanged() {
        let path = scratch("rejected");
        let engine = engine_at(path.clone());

        engine
            .transact(|machine, _now, wall| machine.create("Exam".to_string(), 30 * SECOND, wall))
            .unwrap();
        engine
            .transact(|machine, now, wall| machine.start(now, wall))
            .unwrap();
        engine
            .transact(|machine, now, wall| machine.pause(now, wall))
            .unwrap();

        let before = persistence::load(&path).unwrap();

        // Pausing a paused timer, starting a paused timer and resuming a paused
        // timer twice are all refused, and none of them may move the state.
        assert!(engine.transact(|machine, now, wall| machine.pause(now, wall)).is_err());
        assert!(engine.transact(|machine, now, wall| machine.start(now, wall)).is_err());

        let after = persistence::load(&path).unwrap();
        assert_eq!(before.status, after.status);
        assert_eq!(before.remaining_ms, after.remaining_ms);
        assert_eq!(before.revision, after.revision);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn abort_clears_the_persisted_state() {
        let path = scratch("abort");
        let engine = engine_at(path.clone());

        engine
            .transact(|machine, _now, wall| machine.create("Exam".to_string(), 30 * SECOND, wall))
            .unwrap();
        engine
            .transact(|machine, now, wall| machine.start(now, wall))
            .unwrap();
        engine
            .transact(|machine, _now, wall| {
                machine.abort(wall);
                Ok(())
            })
            .unwrap();

        let snapshot = engine.current();
        assert!(!snapshot.is_armed());
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.remaining_ms, 0);

        // The cleared state is durable, so a restart cannot bring the timer back.
        // An aborted timer reports no armed timer, which the persistence layer
        // deliberately treats as "no timer": `load` returns `None` and the
        // application starts idle exactly as it did before the timer existed.
        assert!(
            persistence::load(&path).is_none(),
            "an aborted timer must be indistinguishable from no timer on restart"
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_running_timer_is_settled_by_the_heartbeat_without_further_commands() {
        let path = scratch("heartbeat");
        let engine = engine_at(path.clone());

        engine
            .transact(|machine, _now, wall| machine.create("Short".to_string(), 20, wall))
            .unwrap();
        engine.transact(|machine, now, wall| machine.start(now, wall)).unwrap();

        assert_eq!(engine.current().status, TimerStatus::Running);

        std::thread::sleep(Duration::from_millis(60));

        let (snapshot, completed) = engine.refresh();
        assert!(completed, "the heartbeat must notice the elapsed deadline");
        assert_eq!(snapshot.status, TimerStatus::Completed);
        assert_eq!(snapshot.remaining_ms, 0);
        assert!(snapshot.overtime_ms >= 0);

        // The completion is durable, so the overtime counter survives a restart.
        assert_eq!(
            persistence::load(&path).unwrap().status,
            TimerStatus::Completed
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_heartbeat_changes_nothing_while_the_timer_is_idle() {
        let path = scratch("idle-heartbeat");
        let engine = engine_at(path.clone());

        engine
            .transact(|machine, _now, wall| machine.create("Exam".to_string(), 60 * SECOND, wall))
            .unwrap();

        let first = engine.current();
        let (second, completed) = engine.refresh();

        assert!(!completed);
        assert_eq!(first, second);
        assert_eq!(first.remaining_ms, 60 * SECOND);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_engine_without_a_store_still_runs_the_machine() {
        let engine = TimerEngine::new();
        assert!(!engine.current().is_armed());

        let snapshot = engine
            .transact(|machine, _now, wall| machine.create("Exam".to_string(), 5 * SECOND, wall))
            .unwrap()
            .0;
        assert!(snapshot.is_armed());
        assert_eq!(snapshot.status, TimerStatus::Idle);
    }
}
