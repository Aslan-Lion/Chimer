//! Authoritative timer state machine.
//!
//! The machine is deliberately free of Tauri, file-system and wall-clock side
//! effects: every method receives the current monotonic instant and the current
//! wall-clock millisecond timestamp as arguments. That keeps the counting logic
//! fully deterministic, so it can be unit-tested without sleeping and without a
//! running application.
//!
//! Timekeeping is *anchor based*, never a decrementing counter. While the timer
//! runs it remembers the remaining duration measured at one monotonic
//! [`Instant`] and derives the live value from `Instant::elapsed`. A busy or
//! throttled UI therefore cannot make the countdown drift.

use std::time::Instant;

/// Maximum supported timer and adjustment length: 99 hours, matching creation limits.
pub const MAX_TIMER_MS: i64 = 99 * 60 * 60 * 1_000;

use serde::{Deserialize, Serialize};

/// The five states a single timer can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerStatus {
    /// Configured but not started; the full duration is displayed.
    Idle,
    /// Counting down.
    Running,
    /// Frozen with the remaining duration preserved for later resumption.
    Paused,
    /// Reached zero; the completion timestamp drives the overtime counter.
    Completed,
    /// Terminated before completing, with the remaining duration preserved.
    Stopped,
}

/// Serialisable form of the timer, written to disk so a running or paused
/// countdown survives an application restart.
///
/// A monotonic [`Instant`] is process-local and meaningless in a new process, so
/// it is never persisted. A running timer records a wall-clock deadline instead
/// and rebuilds its monotonic anchor when it is loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerRecord {
    pub timer_id: Option<String>,
    pub name: String,
    pub duration_ms: i64,
    pub remaining_ms: i64,
    pub status: TimerStatus,
    pub revision: u64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    /// Wall-clock deadline (epoch milliseconds) of a running timer. Only set
    /// while running and only used to rebuild the countdown after a restart.
    pub deadline_at: Option<i64>,
}

/// Typed snapshot broadcast to both windows over `timer://state-changed`.
///
/// Both windows render this exact payload, which is what keeps them identical:
/// neither side ever derives a remaining time of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSnapshot {
    /// `None` when no timer has been configured yet.
    pub timer_id: Option<String>,
    pub name: String,
    pub status: TimerStatus,
    /// The duration the timer was configured with.
    pub duration_ms: i64,
    /// Live remaining time, clamped to zero. Never negative.
    pub remaining_ms: i64,
    /// Fraction of the configured duration already elapsed, 0.0 – 1.0.
    pub progress: f64,
    /// Time elapsed since completion; zero while the timer has not completed.
    pub overtime_ms: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    /// Monotonically increasing revision used to discard stale updates.
    pub revision: u64,
    pub updated_at: i64,
}

impl TimerSnapshot {
    /// Snapshot describing "no timer configured", used before anything is created.
    pub fn empty(now_ms: i64) -> Self {
        Self {
            timer_id: None,
            name: String::new(),
            status: TimerStatus::Idle,
            duration_ms: 0,
            remaining_ms: 0,
            progress: 0.0,
            overtime_ms: 0,
            started_at: None,
            completed_at: None,
            revision: 0,
            updated_at: now_ms,
        }
    }

    /// True when a timer has actually been configured in this session.
    pub fn is_armed(&self) -> bool {
        self.timer_id.is_some()
    }
}

/// The single timer, with its anchors and derived bookkeeping.
#[derive(Debug)]
pub struct TimerMachine {
    timer_id: Option<String>,
    name: String,
    duration_ms: i64,
    status: TimerStatus,
    /// Remaining milliseconds measured at [`Self::anchor`]. In every state other
    /// than `Running` this is simply the frozen remaining time.
    remaining_ms: i64,
    /// Monotonic instant at which `remaining_ms` was last correct. Only set
    /// while running — it is the anchor the countdown is derived from.
    anchor: Option<Instant>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    /// Wall-clock deadline persisted for restart recovery; only set while running.
    deadline_at: Option<i64>,
    revision: u64,
    updated_at: i64,
}

impl TimerMachine {
    /// A machine with no timer configured.
    pub fn empty(now_ms: i64) -> Self {
        Self {
            timer_id: None,
            name: String::new(),
            duration_ms: 0,
            status: TimerStatus::Idle,
            remaining_ms: 0,
            anchor: None,
            started_at: None,
            completed_at: None,
            deadline_at: None,
            revision: 0,
            updated_at: now_ms,
        }
    }

    pub fn status(&self) -> TimerStatus {
        self.status
    }

    /// Rebuilds a machine from a persisted record.
    ///
    /// A record that cannot describe a usable timer (no identity, no positive
    /// duration) is discarded rather than trusted, which is what makes a
    /// corrupted or half-written state file harmless.
    pub fn restore(record: TimerRecord, mono_now: Instant, wall_now: i64) -> Self {
        if record.timer_id.is_none() || record.duration_ms <= 0 {
            return Self::empty(wall_now);
        }

        let duration_ms = record.duration_ms;
        let mut machine = Self {
            timer_id: record.timer_id,
            name: record.name,
            duration_ms,
            status: record.status,
            remaining_ms: record.remaining_ms.clamp(0, MAX_TIMER_MS),
            anchor: None,
            started_at: record.started_at,
            completed_at: record.completed_at,
            deadline_at: record.deadline_at,
            revision: record.revision,
            updated_at: record.updated_at,
        };

        match machine.status {
            TimerStatus::Running => {
                // The countdown is recovered from its wall-clock deadline, so the
                // time the application spent closed is counted correctly instead
                // of being lost or silently restarted. The result is clamped into
                // the configured range so a clock change while the application was
                // closed can never produce a nonsensical remaining time.
                let deadline = machine
                    .deadline_at
                    .unwrap_or(wall_now + machine.remaining_ms);
                machine.remaining_ms = (deadline - wall_now).clamp(0, duration_ms);
                machine.anchor = Some(mono_now);
                if machine.remaining_ms == 0 {
                    machine.status = TimerStatus::Completed;
                    machine.completed_at = Some(deadline);
                    machine.deadline_at = None;
                    machine.anchor = None;
                } else {
                    machine.deadline_at = Some(deadline);
                }
            }
            TimerStatus::Paused | TimerStatus::Stopped => {
                machine.anchor = None;
                machine.deadline_at = None;
            }
            TimerStatus::Completed => {
                machine.remaining_ms = 0;
                machine.anchor = None;
                machine.deadline_at = None;
                if machine.completed_at.is_none() {
                    machine.completed_at = Some(wall_now);
                }
            }
            TimerStatus::Idle => {
                machine.remaining_ms = duration_ms;
                machine.anchor = None;
                machine.started_at = None;
                machine.completed_at = None;
                machine.deadline_at = None;
            }
        }

        machine
    }

    /// Live remaining time at `mono_now`, derived from the monotonic anchor and
    /// clamped to zero.
    pub fn remaining_at(&self, mono_now: Instant) -> i64 {
        match self.anchor {
            Some(anchor) if self.status == TimerStatus::Running => {
                let elapsed = mono_now.saturating_duration_since(anchor).as_millis() as i64;
                (self.remaining_ms - elapsed).max(0)
            }
            _ => self.remaining_ms.max(0),
        }
    }

    /// Time elapsed since the timer completed, derived from the completion
    /// timestamp rather than accumulated tick by tick.
    pub fn overtime_at(&self, wall_now: i64) -> i64 {
        match (self.status, self.completed_at) {
            (TimerStatus::Completed, Some(completed_at)) => (wall_now - completed_at).max(0),
            _ => 0,
        }
    }

    /// Moves a running timer whose deadline has arrived to `Completed`.
    ///
    /// Returns `true` when the state actually changed, so the caller knows to
    /// persist and broadcast exactly once.
    pub fn settle(&mut self, mono_now: Instant, wall_now: i64) -> bool {
        if self.status != TimerStatus::Running || self.remaining_at(mono_now) > 0 {
            return false;
        }

        self.status = TimerStatus::Completed;
        self.remaining_ms = 0;
        self.anchor = None;
        self.completed_at = Some(self.deadline_at.unwrap_or(wall_now));
        self.deadline_at = None;
        self.touch(wall_now);
        true
    }

    /// Immutable snapshot of the current state.
    pub fn snapshot(&self, mono_now: Instant, wall_now: i64) -> TimerSnapshot {
        if !self.is_armed() {
            return TimerSnapshot::empty(wall_now);
        }

        let remaining_ms = self.remaining_at(mono_now);
        let progress = if self.duration_ms <= 0 {
            0.0
        } else {
            ((self.duration_ms - remaining_ms) as f64 / self.duration_ms as f64).clamp(0.0, 1.0)
        };

        TimerSnapshot {
            timer_id: self.timer_id.clone(),
            name: self.name.clone(),
            status: self.status,
            duration_ms: self.duration_ms,
            remaining_ms,
            progress,
            overtime_ms: self.overtime_at(wall_now),
            started_at: self.started_at,
            completed_at: self.completed_at,
            revision: self.revision,
            updated_at: self.updated_at,
        }
    }

    /// Serialisable projection of the current state.
    ///
    /// The wall clock is not needed here: the instant a running countdown is
    /// recovered from is already held in `deadline_at`.
    pub fn to_record(&self, mono_now: Instant) -> TimerRecord {
        TimerRecord {
            timer_id: self.timer_id.clone(),
            name: self.name.clone(),
            duration_ms: self.duration_ms,
            remaining_ms: self.remaining_at(mono_now),
            status: self.status,
            revision: self.revision,
            updated_at: self.updated_at,
            started_at: self.started_at,
            completed_at: self.completed_at,
            deadline_at: self.deadline_at,
        }
    }

    // ------------------------------------------------------------------ actions

    /// Configures a new timer and returns it to `Idle`.
    pub fn create(&mut self, name: String, duration_ms: i64, wall_now: i64) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Give the timer a name before continuing.".to_string());
        }
        if duration_ms <= 0 {
            return Err("Duration must be greater than zero.".to_string());
        }

        self.timer_id = Some(new_timer_id(wall_now));
        self.name = name.trim().to_string();
        self.duration_ms = duration_ms;
        self.remaining_ms = duration_ms;
        self.status = TimerStatus::Idle;
        self.anchor = None;
        self.started_at = None;
        self.completed_at = None;
        self.deadline_at = None;
        self.touch(wall_now);
        Ok(())
    }

    /// Begins the countdown from the full configured duration.
    ///
    /// A stopped timer restarts from scratch — that is the defined reset/restart
    /// behaviour for the `Stopped` state. A completed timer must be reset first,
    /// because its overtime counter is still running.
    pub fn start(&mut self, mono_now: Instant, wall_now: i64) -> Result<(), String> {
        match self.status {
            TimerStatus::Idle => {}
            TimerStatus::Stopped => self.remaining_ms = self.duration_ms,
            TimerStatus::Running => return Err("The timer is already running.".to_string()),
            TimerStatus::Paused => {
                return Err("Resume the paused timer instead of starting it again.".to_string());
            }
            TimerStatus::Completed => {
                return Err("Reset the timer before starting it again.".to_string());
            }
        }

        self.started_at = Some(wall_now);
        self.begin_run(mono_now, wall_now);
        Ok(())
    }

    /// Freezes the countdown, preserving the exact remaining duration.
    pub fn pause(&mut self, mono_now: Instant, wall_now: i64) -> Result<(), String> {
        if self.status != TimerStatus::Running {
            return Err("Only a running timer can be paused.".to_string());
        }

        self.remaining_ms = self.remaining_at(mono_now);
        self.anchor = None;
        self.deadline_at = None;
        self.status = TimerStatus::Paused;
        self.touch(wall_now);
        Ok(())
    }

    /// Continues a paused countdown from a fresh anchor, so no time is added or
    /// lost across the pause.
    pub fn resume(&mut self, mono_now: Instant, wall_now: i64) -> Result<(), String> {
        if self.status != TimerStatus::Paused {
            return Err("Only a paused timer can be resumed.".to_string());
        }
        if self.remaining_ms <= 0 {
            return Err("The timer has no time left; reset it to start again.".to_string());
        }

        self.begin_run(mono_now, wall_now);
        Ok(())
    }

    /// Restores the original configured duration and returns the timer to `Idle`.
    pub fn reset(&mut self, wall_now: i64) -> Result<(), String> {
        if !self.is_armed() {
            return Err("No timer has been created yet.".to_string());
        }

        self.remaining_ms = self.duration_ms;
        self.status = TimerStatus::Idle;
        self.anchor = None;
        self.started_at = None;
        self.completed_at = None;
        self.deadline_at = None;
        self.touch(wall_now);
        Ok(())
    }

    /// Aborts the timer outright and returns the machine to the empty `Idle`
    /// state.
    ///
    /// Unlike [`Self::reset`], which keeps the configured timer and restores its
    /// full duration, aborting discards it: the machine reports no armed timer,
    /// so both windows fall back to the idle presentation — `00:00`, no elapsed
    /// progress, no overtime counter and no completion timestamp — and nothing
    /// is left for a restart to recover. It is deliberately infallible so an
    /// abort is never refused, whichever state the timer happens to be in.
    pub fn abort(&mut self, wall_now: i64) {
        *self = Self::empty(wall_now);
    }

    /// Terminates an active countdown, preserving the remaining duration.
    pub fn stop(&mut self, mono_now: Instant, wall_now: i64) -> Result<(), String> {
        if !matches!(self.status, TimerStatus::Running | TimerStatus::Paused) {
            return Err("Only a running or paused timer can be stopped.".to_string());
        }

        self.remaining_ms = self.remaining_at(mono_now);
        self.anchor = None;
        self.deadline_at = None;
        self.status = TimerStatus::Stopped;
        self.touch(wall_now);
        Ok(())
    }

    /// Adds or subtracts time while running, paused or completed. The signed
    /// delta is validated here as well as at the IPC boundary because this is
    /// the authoritative state transition.
    pub fn adjust(
        &mut self,
        delta_ms: i64,
        mono_now: Instant,
        wall_now: i64,
    ) -> Result<(), String> {
        let magnitude = delta_ms
            .checked_abs()
            .ok_or_else(|| "Timer adjustment is outside the supported range.".to_string())?;
        if magnitude == 0 || magnitude > MAX_TIMER_MS {
            return Err("Timer adjustment must be between 1 ms and 99 hours.".to_string());
        }
        if !matches!(
            self.status,
            TimerStatus::Running | TimerStatus::Paused | TimerStatus::Completed
        ) {
            return Err("Only a running, paused or completed timer can be adjusted.".to_string());
        }

        if self.status == TimerStatus::Completed {
            let overtime = self.overtime_at(wall_now);
            self.anchor = None;
            self.deadline_at = None;

            if delta_ms > 0 {
                self.remaining_ms = delta_ms;
                self.status = TimerStatus::Paused;
                self.completed_at = None;
            } else if magnitude <= overtime {
                self.completed_at = Some(
                    self.completed_at
                        .expect("completed timers have a completion timestamp")
                        + magnitude,
                );
            } else {
                self.remaining_ms = magnitude - overtime;
                self.status = TimerStatus::Paused;
                self.completed_at = None;
            }

            self.touch(wall_now);
            return Ok(());
        }

        let remaining = self.remaining_at(mono_now);
        let adjusted = remaining
            .checked_add(delta_ms)
            .ok_or_else(|| "Timer adjustment would overflow.".to_string())?;
        if adjusted > MAX_TIMER_MS {
            return Err("Remaining time cannot exceed 99 hours.".to_string());
        }
        let deadline = if self.status == TimerStatus::Running {
            Some(
                wall_now
                    .checked_add(adjusted.max(0))
                    .ok_or_else(|| "Timer deadline would overflow.".to_string())?,
            )
        } else {
            None
        };

        self.remaining_ms = adjusted.max(0);
        if self.status == TimerStatus::Running {
            self.anchor = Some(mono_now);
            self.deadline_at = deadline;
        }
        if self.remaining_ms == 0 {
            self.status = TimerStatus::Completed;
            self.anchor = None;
            self.deadline_at = None;
            self.completed_at = Some(wall_now);
        }
        self.touch(wall_now);
        Ok(())
    }

    // ----------------------------------------------------------------- internals

    /// Arms the countdown from the current remaining time.
    fn begin_run(&mut self, mono_now: Instant, wall_now: i64) {
        if self.remaining_ms <= 0 {
            self.remaining_ms = self.duration_ms;
        }
        self.anchor = Some(mono_now);
        self.deadline_at = Some(wall_now + self.remaining_ms);
        self.status = TimerStatus::Running;
        self.completed_at = None;
        self.touch(wall_now);
    }

    fn is_armed(&self) -> bool {
        self.timer_id.is_some()
    }

    fn touch(&mut self, wall_now: i64) {
        self.revision += 1;
        self.updated_at = wall_now;
    }
}

/// Identifier for a timer. The wall clock plus a process-local counter is enough
/// to make the id unique without pulling in a UUID dependency.
fn new_timer_id(wall_now: i64) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("timer-{wall_now}-{sequence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const SECOND: i64 = 1_000;

    /// Fixed monotonic origin so every test is deterministic without sleeping.
    fn origin() -> Instant {
        Instant::now()
    }

    fn machine(duration_ms: i64, wall_now: i64) -> TimerMachine {
        let mut machine = TimerMachine::empty(wall_now);
        machine
            .create("Examination".to_string(), duration_ms, wall_now)
            .expect("a positive duration is valid");
        machine
    }

    fn completed_machine(overtime_ms: i64) -> (TimerMachine, Instant, i64) {
        let start = origin();
        let duration_ms = 60 * SECOND;
        let mut machine = machine(duration_ms, 0);
        machine.start(start, 0).unwrap();

        let completed_at = start + Duration::from_millis(duration_ms as u64);
        assert!(machine.settle(completed_at, duration_ms));

        (machine, completed_at, duration_ms + overtime_ms)
    }

    /// A record is only ever built through the machine, so the tests exercise
    /// the real persisted shape.
    fn record_of(machine: &TimerMachine, mono_now: Instant) -> TimerRecord {
        machine.to_record(mono_now)
    }

    #[test]
    fn create_arms_an_idle_timer_with_the_full_duration() {
        let machine = machine(25 * 60 * SECOND, 1_000);
        let snapshot = machine.snapshot(origin(), 1_000);

        assert!(snapshot.is_armed());
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.duration_ms, 25 * 60 * SECOND);
        assert_eq!(snapshot.remaining_ms, 25 * 60 * SECOND);
        assert_eq!(snapshot.progress, 0.0);
        assert_eq!(snapshot.overtime_ms, 0);
    }

    #[test]
    fn create_rejects_empty_names_and_non_positive_durations() {
        let mut machine = TimerMachine::empty(0);

        assert!(machine.create("   ".to_string(), SECOND, 0).is_err());
        assert!(machine.create("Exam".to_string(), 0, 0).is_err());
        assert!(machine.create("Exam".to_string(), -SECOND, 0).is_err());
        assert!(machine.to_record(origin()).timer_id.is_none());
    }

    #[test]
    fn running_countdown_is_derived_from_the_monotonic_anchor() {
        let start = origin();
        let mut machine = machine(10 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();

        assert_eq!(machine.remaining_at(start), 10 * SECOND);
        assert_eq!(machine.remaining_at(start + Duration::from_millis(2_500)), 7_500);
        assert_eq!(machine.remaining_at(start + Duration::from_millis(9_999)), 1);
    }

    #[test]
    fn long_elapsed_jumps_do_not_accumulate_drift() {
        let start = origin();
        let mut machine = machine(6 * 60 * 60 * SECOND, 0);
        machine.start(start, 0).unwrap();

        // One huge jump forward must produce exactly the elapsed time, which is
        // impossible for a counter that is decremented once per interval.
        let after = machine.remaining_at(start + Duration::from_secs(3_600));
        assert_eq!(after, 5 * 60 * 60 * SECOND);
    }

    #[test]
    fn remaining_is_clamped_to_zero_and_completion_is_automatic() {
        let start = origin();
        let mut machine = machine(SECOND, 5_000);
        machine.start(start, 5_000).unwrap();

        let beyond = start + Duration::from_secs(3);
        assert_eq!(machine.remaining_at(beyond), 0);
        assert!(machine.settle(beyond, 8_000));

        let snapshot = machine.snapshot(beyond, 8_000);
        assert_eq!(snapshot.status, TimerStatus::Completed);
        assert_eq!(snapshot.remaining_ms, 0);
        assert_eq!(snapshot.progress, 1.0);
        assert_eq!(snapshot.completed_at, Some(6_000));
        assert_eq!(snapshot.overtime_ms, 2_000);
    }

    #[test]
    fn settle_reports_a_change_only_once() {
        let start = origin();
        let mut machine = machine(SECOND, 0);
        machine.start(start, 0).unwrap();

        let beyond = start + Duration::from_secs(2);
        assert!(machine.settle(beyond, 2_000));
        assert!(!machine.settle(beyond, 2_000));

        let revision = machine.snapshot(beyond, 2_000).revision;
        assert!(!machine.settle(beyond, 9_000));
        assert_eq!(machine.snapshot(beyond, 9_000).revision, revision);
    }

    #[test]
    fn pause_freezes_the_remaining_time_exactly() {
        let start = origin();
        let mut machine = machine(60 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();
        machine.pause(start + Duration::from_millis(7_250), 8_250).unwrap();

        let frozen = machine.snapshot(start + Duration::from_millis(7_250), 8_250);
        assert_eq!(frozen.status, TimerStatus::Paused);
        assert_eq!(frozen.remaining_ms, 52_750);

        // Any amount of wall time passing while paused cannot change the value.
        let later = start + Duration::from_secs(600);
        assert_eq!(machine.remaining_at(later), 52_750);
        assert_eq!(machine.snapshot(later, 600_000).remaining_ms, 52_750);
    }

    #[test]
    fn resume_continues_from_a_new_anchor_without_gaining_or_losing_time() {
        let start = origin();
        let mut machine = machine(60 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();
        machine.pause(start + Duration::from_millis(7_250), 8_250).unwrap();

        let resumed_at = start + Duration::from_secs(300);
        machine.resume(resumed_at, 301_000).unwrap();

        assert_eq!(machine.remaining_at(resumed_at), 52_750);
        assert_eq!(
            machine.remaining_at(resumed_at + Duration::from_millis(2_750)),
            50_000
        );
    }

    #[test]
    fn reset_restores_the_original_duration_and_the_idle_state() {
        let start = origin();
        let mut machine = machine(2 * 60 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();
        machine
            .pause(start + Duration::from_secs(30), 31_000)
            .unwrap();
        machine.reset(32_000).unwrap();

        let snapshot = machine.snapshot(start + Duration::from_secs(30), 32_000);
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.remaining_ms, 2 * 60 * SECOND);
        assert_eq!(snapshot.progress, 0.0);
        assert_eq!(snapshot.overtime_ms, 0);
    }

    #[test]
    fn stop_preserves_the_remaining_duration_for_inspection() {
        let start = origin();
        let mut machine = machine(90 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();
        machine
            .stop(start + Duration::from_millis(10_500), 11_500)
            .unwrap();

        let snapshot = machine.snapshot(start + Duration::from_millis(10_500), 11_500);
        assert_eq!(snapshot.status, TimerStatus::Stopped);
        assert_eq!(snapshot.remaining_ms, 79_500);

        // A stopped timer restarts from the full configured duration.
        machine.start(start + Duration::from_secs(60), 61_000).unwrap();
        let restarted = machine.snapshot(start + Duration::from_secs(60), 61_000);
        assert_eq!(restarted.status, TimerStatus::Running);
        assert_eq!(restarted.remaining_ms, 90 * SECOND);
    }

    #[test]
    fn stopping_a_paused_timer_keeps_the_frozen_remaining_time() {
        let start = origin();
        let mut machine = machine(45 * SECOND, 0);
        machine.start(start, 0).unwrap();
        machine.pause(start + Duration::from_secs(5), 5_000).unwrap();
        machine.stop(start + Duration::from_secs(500), 500_000).unwrap();

        let snapshot = machine.snapshot(start + Duration::from_secs(500), 500_000);
        assert_eq!(snapshot.status, TimerStatus::Stopped);
        assert_eq!(snapshot.remaining_ms, 40 * SECOND);
    }

    #[test]
    fn invalid_transitions_are_rejected_without_mutating_state() {
        let start = origin();
        let mut machine = machine(30 * SECOND, 0);

        // Nothing to pause / resume / stop while idle.
        assert!(machine.pause(start, 0).is_err());
        assert!(machine.resume(start, 0).is_err());
        assert!(machine.stop(start, 0).is_err());

        machine.start(start, 0).unwrap();
        let revision = machine.snapshot(start, 0).revision;

        // Repeated commands cannot corrupt a running timer.
        assert!(machine.start(start, 0).is_err());
        assert!(machine.resume(start, 0).is_err());
        assert_eq!(machine.snapshot(start, 0).revision, revision);
        assert_eq!(machine.snapshot(start, 0).status, TimerStatus::Running);

        machine.pause(start, 0).unwrap();
        // Repeated or out-of-order commands on a paused timer are refused too.
        assert!(machine.pause(start, 0).is_err());
        assert!(machine.start(start, 0).is_err());
        assert_eq!(machine.snapshot(start, 0).status, TimerStatus::Paused);
    }

    #[test]
    fn a_completed_timer_must_be_reset_before_it_can_run_again() {
        let start = origin();
        let mut machine = machine(SECOND, 0);
        machine.start(start, 0).unwrap();
        let beyond = start + Duration::from_secs(1);
        assert!(machine.settle(beyond, 1_000));

        assert!(machine.start(beyond, 1_000).is_err());
        assert!(machine.pause(beyond, 1_000).is_err());
        assert!(machine.stop(beyond, 1_000).is_err());

        machine.reset(1_000).unwrap();
        assert_eq!(machine.snapshot(beyond, 1_000).status, TimerStatus::Idle);
        machine.start(beyond, 1_000).unwrap();
        assert_eq!(machine.snapshot(beyond, 1_000).status, TimerStatus::Running);
    }

    #[test]
    fn overtime_grows_from_the_completion_timestamp() {
        let start = origin();
        let mut machine = machine(5 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();
        let beyond = start + Duration::from_secs(6);
        assert!(machine.settle(beyond, 7_000));

        assert_eq!(machine.snapshot(beyond, 7_000).overtime_ms, 1_000);
        assert_eq!(machine.snapshot(beyond, 7_500).overtime_ms, 1_500);
        assert_eq!(machine.snapshot(beyond, 37_000).overtime_ms, 31 * SECOND);
        // The main countdown stays at zero rather than going negative.
        assert_eq!(machine.snapshot(beyond, 37_000).remaining_ms, 0);
    }

    #[test]
    fn resetting_clears_the_overtime_counter() {
        let start = origin();
        let mut machine = machine(SECOND, 0);
        machine.start(start, 0).unwrap();
        let beyond = start + Duration::from_secs(1);
        machine.settle(beyond, 1_000);
        assert_eq!(machine.snapshot(beyond, 20_000).overtime_ms, 19_000);

        machine.reset(20_000).unwrap();
        assert_eq!(machine.snapshot(beyond, 20_000).overtime_ms, 0);
    }

    #[test]
    fn abort_clears_a_running_timer_to_the_empty_idle_state() {
        let start = origin();
        let mut machine = machine(60 * SECOND, 1_000);
        machine.start(start, 1_000).unwrap();
        machine.abort(2_000);

        let snapshot = machine.snapshot(start + Duration::from_secs(5), 7_000);
        assert!(!snapshot.is_armed());
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.remaining_ms, 0);
        assert_eq!(snapshot.progress, 0.0);
        assert_eq!(snapshot.overtime_ms, 0);
        assert_eq!(snapshot.started_at, None);
        assert_eq!(snapshot.completed_at, None);
    }

    #[test]
    fn abort_clears_the_completed_state_and_the_overtime_counter() {
        let start = origin();
        let mut machine = machine(SECOND, 0);
        machine.start(start, 0).unwrap();
        let beyond = start + Duration::from_secs(1);
        machine.settle(beyond, 1_000);
        assert_eq!(machine.snapshot(beyond, 20_000).overtime_ms, 19_000);

        machine.abort(20_000);
        let snapshot = machine.snapshot(beyond, 20_000);
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.overtime_ms, 0);
        assert_eq!(snapshot.remaining_ms, 0);
        assert_eq!(snapshot.progress, 0.0);
    }

    #[test]
    fn aborting_leaves_nothing_for_a_restart_to_recover() {
        let mut machine = machine(30 * SECOND, 0);
        let start = origin();
        machine.start(start, 0).unwrap();
        machine.abort(1_000);

        let restored = TimerMachine::restore(machine.to_record(start), origin(), 500_000);
        let snapshot = restored.snapshot(origin(), 500_000);
        assert!(!snapshot.is_armed());
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.remaining_ms, 0);
    }

    #[test]
    fn revision_increases_on_every_valid_transition() {
        let start = origin();
        let mut machine = machine(10 * SECOND, 0);
        let created = machine.snapshot(start, 0).revision;

        machine.start(start, 0).unwrap();
        let started = machine.snapshot(start, 0).revision;
        machine.pause(start, 0).unwrap();
        let paused = machine.snapshot(start, 0).revision;
        machine.resume(start, 0).unwrap();
        let resumed = machine.snapshot(start, 0).revision;
        machine.stop(start, 0).unwrap();
        let stopped = machine.snapshot(start, 0).revision;

        assert!(created < started && started < paused && paused < resumed && resumed < stopped);
    }

    #[test]
    fn short_durations_complete_on_schedule() {
        let start = origin();
        let mut machine = machine(50, 0);
        machine.start(start, 0).unwrap();

        assert_eq!(machine.remaining_at(start + Duration::from_millis(49)), 1);
        assert!(!machine.settle(start + Duration::from_millis(49), 49));
        assert_eq!(machine.remaining_at(start + Duration::from_millis(50)), 0);
        assert!(machine.settle(start + Duration::from_millis(50), 50));
        assert_eq!(machine.snapshot(start + Duration::from_millis(50), 50).status, TimerStatus::Completed);
    }

    #[test]
    fn recovery_of_a_running_timer_counts_the_time_spent_closed() {
        let mut live = machine(10 * 60 * SECOND, 100_000);
        let start = origin();
        live.start(start, 100_000).unwrap();
        let record = record_of(&live, start);

        // Reopen the application 5 minutes and 2 seconds of wall time later.
        let restored_at = origin();
        let reopened = 402_000;
        let restored = TimerMachine::restore(record, restored_at, reopened);

        let snapshot = restored.snapshot(restored_at, reopened);
        assert_eq!(snapshot.status, TimerStatus::Running);
        assert_eq!(snapshot.remaining_ms, 10 * 60 * SECOND - 302 * SECOND);

        // The rebuilt timer keeps counting from the restored anchor.
        let snapshot = restored.snapshot(restored_at + Duration::from_secs(60), reopened + 60_000);
        assert_eq!(snapshot.remaining_ms, 10 * 60 * SECOND - 362 * SECOND);
    }

    #[test]
    fn recovery_of_a_running_timer_past_its_deadline_completes_with_overtime() {
        let mut live = machine(60 * SECOND, 0);
        let start = origin();
        live.start(start, 0).unwrap();
        let record = record_of(&live, start);

        let restored_at = origin();
        let reopened = 5 * 60 * SECOND;
        let restored = TimerMachine::restore(record, restored_at, reopened);

        let snapshot = restored.snapshot(restored_at, reopened);
        assert_eq!(snapshot.status, TimerStatus::Completed);
        assert_eq!(snapshot.remaining_ms, 0);
        // Overtime is measured from the real deadline, not from the reopen time.
        assert_eq!(snapshot.overtime_ms, 4 * 60 * SECOND);
    }

    #[test]
    fn recovery_of_a_paused_timer_restores_the_frozen_remaining_time() {
        let start = origin();
        let mut live = machine(30 * 60 * SECOND, 0);
        live.start(start, 0).unwrap();
        live.pause(start + Duration::from_secs(600), 600_000).unwrap();
        let record = record_of(&live, start + Duration::from_secs(600));

        let reopened_at = origin();
        let restored = TimerMachine::restore(record, reopened_at, 9_000_000);

        let snapshot = restored.snapshot(reopened_at, 9_000_000);
        assert_eq!(snapshot.status, TimerStatus::Paused);
        assert_eq!(snapshot.remaining_ms, 30 * 60 * SECOND - 600 * SECOND);
        // A restored paused timer must not resume on its own.
        assert_eq!(
            restored.snapshot(reopened_at + Duration::from_secs(120), 9_120_000).remaining_ms,
            30 * 60 * SECOND - 600 * SECOND
        );
    }

    #[test]
    fn a_stopped_timer_is_never_restarted_silently() {
        let start = origin();
        let mut live = machine(10 * SECOND, 0);
        live.start(start, 0).unwrap();
        live.stop(start + Duration::from_secs(2), 2_000).unwrap();
        let record = record_of(&live, start + Duration::from_secs(2));

        let restored = TimerMachine::restore(record, origin(), 500_000);
        let snapshot = restored.snapshot(origin(), 500_000);
        assert_eq!(snapshot.status, TimerStatus::Stopped);
        assert_eq!(snapshot.remaining_ms, 8 * SECOND);
    }

    #[test]
    fn a_completed_timer_keeps_counting_overtime_across_a_restart() {
        let start = origin();
        let mut live = machine(SECOND, 0);
        live.start(start, 0).unwrap();
        live.settle(start + Duration::from_secs(1), 1_000);
        let record = record_of(&live, start + Duration::from_secs(1));

        let restored = TimerMachine::restore(record, origin(), 61_000);
        let snapshot = restored.snapshot(origin(), 61_000);
        assert_eq!(snapshot.status, TimerStatus::Completed);
        assert_eq!(snapshot.overtime_ms, 60 * SECOND);
    }

    #[test]
    fn an_idle_timer_is_restored_with_its_full_duration() {
        let live = machine(15 * SECOND, 0);
        let record = record_of(&live, origin());

        let restored = TimerMachine::restore(record, origin(), 900_000);
        let snapshot = restored.snapshot(origin(), 900_000);
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.remaining_ms, 15 * SECOND);
    }

    #[test]
    fn unusable_records_are_discarded_instead_of_trusted() {
        let now = origin();

        let mut nameless = machine(SECOND, 0).to_record(now);
        nameless.timer_id = None;
        assert!(!TimerMachine::restore(nameless, now, 0).snapshot(now, 0).is_armed());

        let mut zero = machine(SECOND, 0).to_record(now);
        zero.duration_ms = 0;
        assert!(!TimerMachine::restore(zero, now, 0).snapshot(now, 0).is_armed());

        let mut negative = machine(SECOND, 0).to_record(now);
        negative.duration_ms = -5_000;
        assert!(!TimerMachine::restore(negative, now, 0).snapshot(now, 0).is_armed());
    }

    #[test]
    fn a_corrupted_remaining_value_is_clamped_into_range() {
        let now = origin();
        let mut record = machine(60 * SECOND, 0).to_record(now);
        record.remaining_ms = 10 * 60 * SECOND;
        assert_eq!(
            TimerMachine::restore(record, now, 0).snapshot(now, 0).remaining_ms,
            60 * SECOND
        );

        let mut record = machine(60 * SECOND, 0).to_record(now);
        record.remaining_ms = -1;
        record.status = TimerStatus::Paused;
        assert_eq!(TimerMachine::restore(record, now, 0).snapshot(now, 0).remaining_ms, 0);
    }

    #[test]
    fn a_running_record_without_a_deadline_falls_back_to_the_stored_remaining_time() {
        let now = origin();
        let mut record = machine(30 * SECOND, 0).to_record(now);
        record.status = TimerStatus::Running;
        record.remaining_ms = 12 * SECOND;
        record.deadline_at = None;

        let restored = TimerMachine::restore(record, now, 100_000);
        let snapshot = restored.snapshot(now, 100_000);
        assert_eq!(snapshot.status, TimerStatus::Running);
        assert_eq!(snapshot.remaining_ms, 12 * SECOND);
    }

    #[test]
    fn an_empty_machine_reports_no_timer() {
        let snapshot = TimerMachine::empty(1_234).snapshot(origin(), 1_234);
        assert!(!snapshot.is_armed());
        assert_eq!(snapshot.remaining_ms, 0);
        assert_eq!(snapshot.overtime_ms, 0);
        assert_eq!(snapshot.progress, 0.0);
    }

    #[test]
    fn progress_tracks_elapsed_time() {
        let start = origin();
        let mut machine = machine(100 * SECOND, 0);
        machine.start(start, 0).unwrap();

        assert!((machine.snapshot(start, 0).progress - 0.0).abs() < f64::EPSILON);
        assert!((machine.snapshot(start + Duration::from_secs(25), 0).progress - 0.25).abs() < 1e-9);
        assert!((machine.snapshot(start + Duration::from_secs(100), 0).progress - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_timer_created_while_another_is_running_replaces_it_cleanly() {
        let start = origin();
        let mut machine = machine(60 * SECOND, 0);
        machine.start(start, 0).unwrap();
        let first = machine.snapshot(start, 0).timer_id;

        machine.create("Second".to_string(), 90 * SECOND, 5_000).unwrap();
        let snapshot = machine.snapshot(start, 5_000);

        assert_ne!(snapshot.timer_id, first);
        assert_eq!(snapshot.status, TimerStatus::Idle);
        assert_eq!(snapshot.remaining_ms, 90 * SECOND);
        assert_eq!(snapshot.overtime_ms, 0);
    }

    #[test]
    fn running_timer_adjustments_move_the_deadline_and_keep_progress_consistent() {
        let start = origin();
        let mut machine = machine(100 * SECOND, 10_000);
        machine.start(start, 10_000).unwrap();

        machine
            .adjust(20 * SECOND, start + Duration::from_secs(10), 20_000)
            .unwrap();
        let increased = machine.snapshot(start + Duration::from_secs(10), 20_000);
        assert_eq!(increased.status, TimerStatus::Running);
        assert_eq!(increased.remaining_ms, 110 * SECOND);
        assert_eq!(machine.deadline_at, Some(130_000));
        assert_eq!(increased.progress, 0.0);

        machine
            .adjust(-30 * SECOND, start + Duration::from_secs(10), 20_000)
            .unwrap();
        let decreased = machine.snapshot(start + Duration::from_secs(10), 20_000);
        assert_eq!(decreased.remaining_ms, 80 * SECOND);
        assert_eq!(machine.deadline_at, Some(100_000));
        assert!((decreased.progress - 0.2).abs() < 1e-9);
    }

    #[test]
    fn paused_timer_adjustments_change_frozen_remaining_without_resuming() {
        let start = origin();
        let mut machine = machine(100 * SECOND, 0);
        machine.start(start, 0).unwrap();
        machine
            .pause(start + Duration::from_secs(10), 10_000)
            .unwrap();

        machine
            .adjust(5 * SECOND, start + Duration::from_secs(20), 20_000)
            .unwrap();
        assert_eq!(machine.status(), TimerStatus::Paused);
        assert_eq!(
            machine
                .snapshot(start + Duration::from_secs(30), 30_000)
                .remaining_ms,
            95 * SECOND
        );
        assert_eq!(machine.deadline_at, None);

        machine
            .adjust(-15 * SECOND, start + Duration::from_secs(30), 30_000)
            .unwrap();
        let snapshot = machine.snapshot(start + Duration::from_secs(40), 40_000);
        assert_eq!(snapshot.status, TimerStatus::Paused);
        assert_eq!(snapshot.remaining_ms, 80 * SECOND);
        assert!((snapshot.progress - 0.2).abs() < 1e-9);
    }

    #[test]
    fn increasing_a_completed_timer_pauses_it_with_zero_overtime() {
        let (mut machine, now, wall_now) = completed_machine(3 * 60 * SECOND);

        machine.adjust(5 * 60 * SECOND, now, wall_now).unwrap();

        let snapshot = machine.snapshot(now, wall_now);
        assert_eq!(snapshot.status, TimerStatus::Paused);
        assert_eq!(snapshot.remaining_ms, 5 * 60 * SECOND);
        assert_eq!(snapshot.overtime_ms, 0);
        assert_eq!(snapshot.completed_at, None);
        assert_eq!(machine.deadline_at, None);

        let restored = TimerMachine::restore(record_of(&machine, now), now, wall_now);
        let restored_snapshot = restored.snapshot(now, wall_now);
        assert_eq!(restored_snapshot.status, TimerStatus::Paused);
        assert_eq!(restored_snapshot.remaining_ms, 5 * 60 * SECOND);
    }

    #[test]
    fn decreasing_a_completed_timer_reduces_overtime_first() {
        let (mut machine, now, wall_now) = completed_machine(3 * 60 * SECOND);

        machine.adjust(-60 * SECOND, now, wall_now).unwrap();

        let snapshot = machine.snapshot(now, wall_now);
        assert_eq!(snapshot.status, TimerStatus::Completed);
        assert_eq!(snapshot.remaining_ms, 0);
        assert_eq!(snapshot.overtime_ms, 2 * 60 * SECOND);
        assert_eq!(snapshot.completed_at, Some(wall_now - 2 * 60 * SECOND));
    }

    #[test]
    fn decreasing_completed_timer_by_exact_overtime_keeps_it_completed_at_zero() {
        let (mut machine, now, wall_now) = completed_machine(3 * 60 * SECOND);

        machine.adjust(-3 * 60 * SECOND, now, wall_now).unwrap();

        let snapshot = machine.snapshot(now, wall_now);
        assert_eq!(snapshot.status, TimerStatus::Completed);
        assert_eq!(snapshot.remaining_ms, 0);
        assert_eq!(snapshot.overtime_ms, 0);
        assert_eq!(snapshot.completed_at, Some(wall_now));
    }

    #[test]
    fn decreasing_completed_timer_past_overtime_pauses_with_excess_remaining() {
        let (mut machine, now, wall_now) = completed_machine(3 * 60 * SECOND);

        machine.adjust(-5 * 60 * SECOND, now, wall_now).unwrap();

        let snapshot = machine.snapshot(now, wall_now);
        assert_eq!(snapshot.status, TimerStatus::Paused);
        assert_eq!(snapshot.remaining_ms, 2 * 60 * SECOND);
        assert_eq!(snapshot.overtime_ms, 0);
        assert_eq!(snapshot.completed_at, None);
        assert_eq!(machine.deadline_at, None);

        let restored = TimerMachine::restore(record_of(&machine, now), now, wall_now);
        let restored_snapshot = restored.snapshot(now, wall_now);
        assert_eq!(restored_snapshot.status, TimerStatus::Paused);
        assert_eq!(restored_snapshot.remaining_ms, 2 * 60 * SECOND);
    }

    #[test]
    fn reducing_a_running_or_paused_timer_to_zero_completes_it() {
        let start = origin();
        let mut running = machine(10 * SECOND, 0);
        running.start(start, 0).unwrap();
        running.adjust(-10 * SECOND, start, 0).unwrap();
        let completed = running.snapshot(start, 0);
        assert_eq!(completed.status, TimerStatus::Completed);
        assert_eq!(completed.remaining_ms, 0);
        assert_eq!(completed.progress, 1.0);
        assert_eq!(completed.completed_at, Some(0));
        assert_eq!(running.deadline_at, None);

        let mut paused = machine(10 * SECOND, 0);
        paused.start(start, 0).unwrap();
        paused.pause(start, 0).unwrap();
        paused.adjust(-10 * SECOND, start, 7).unwrap();
        assert_eq!(paused.status(), TimerStatus::Completed);
        assert_eq!(paused.snapshot(start, 7).completed_at, Some(7));
    }

    #[test]
    fn invalid_adjustments_and_states_are_rejected_without_mutating_the_timer() {
        let now = origin();
        let mut idle = machine(10 * SECOND, 0);
        let idle_before = idle.snapshot(now, 0);
        for delta in [0, i64::MIN, MAX_TIMER_MS + 1] {
            assert!(idle.adjust(delta, now, 0).is_err());
        }
        assert!(idle.adjust(1, now, 0).is_err());
        assert_eq!(idle.status(), TimerStatus::Idle);
        assert_eq!(idle.snapshot(now, 0), idle_before);

        idle.start(now, 0).unwrap();
        idle.pause(now, 0).unwrap();
        let before = idle.snapshot(now, 0);
        assert!(idle.adjust(MAX_TIMER_MS, now, 1).is_err());
        assert_eq!(idle.snapshot(now, 0), before);
        idle.adjust(-20 * SECOND, now, 1).unwrap();
        assert_eq!(idle.status(), TimerStatus::Completed);
        idle.adjust(SECOND, now, 2)
            .expect("completed timers can be adjusted into a paused countdown");
        let adjusted_completed = idle.snapshot(now, 2);
        assert_eq!(adjusted_completed.status, TimerStatus::Paused);
        assert_eq!(adjusted_completed.remaining_ms, SECOND);
        assert_eq!(adjusted_completed.overtime_ms, 0);

        let mut stopped = machine(10 * SECOND, 0);
        stopped.start(now, 0).unwrap();
        stopped.stop(now, 1).unwrap();
        let stopped_before = stopped.snapshot(now, 1);
        assert!(stopped.adjust(SECOND, now, 2).is_err());
        assert_eq!(stopped.snapshot(now, 2), stopped_before);
    }
}
