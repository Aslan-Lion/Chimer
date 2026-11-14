//! Authoritative session cursor.
//!
//! Like [`crate::timer::state`], this is deliberately free of Tauri, the file
//! system and the monotonic clock: every method receives the current wall-clock
//! millisecond timestamp, so the stage-advance rules are deterministic and
//! testable without sleeping or a running application.
//!
//! The cursor never counts anything down itself. The *running* stage is an
//! ordinary timer owned by [`crate::timer`]; this module only records which
//! stage that timer belongs to, whether the session is waiting for the operator
//! and whether a transition countdown is elapsing.

use serde::{Deserialize, Serialize};

/// How the session moves from one stage to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StageTransition {
    /// The next stage starts on its own once the current one completes.
    #[default]
    Auto,
    /// The session waits for the operator before starting the next stage.
    Manual,
}

/// The resolved message preset a stage shows on the presentation display.
///
/// The frontend resolves a stored message id into this payload when the session
/// starts, so Rust can present the message on its own — the display window never
/// has to wait for the control window to forward it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StageMessage {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub font_size: f64,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub background_color: Option<String>,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub animation: String,
    /// How long the message stays on screen, in milliseconds; `0` is sticky.
    #[serde(default)]
    pub duration_ms: i64,
}

/// One stage of a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStage {
    pub id: String,
    pub name: String,
    pub duration_ms: i64,
    /// The message shown while this stage runs, if any.
    #[serde(default)]
    pub message: Option<StageMessage>,
    /// Warn when one of these remaining-time thresholds (ms) is crossed.
    #[serde(default)]
    pub warning_intervals_ms: Vec<i64>,
    /// Whether a sound should play when this stage starts and its warnings fire.
    #[serde(default)]
    pub sound_enabled: bool,
    #[serde(default)]
    pub transition: StageTransition,
    /// Optional pause between this stage and the next, in milliseconds.
    #[serde(default)]
    pub transition_countdown_ms: Option<i64>,
}

impl SessionStage {
    /// A stage is usable only when it names something and lasts a positive
    /// amount of time; anything else is dropped before a session starts.
    pub fn is_usable(&self) -> bool {
        !self.name.trim().is_empty() && self.duration_ms > 0
    }
}

/// The session as configured by the operator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub stages: Vec<SessionStage>,
}

impl SessionDefinition {
    /// Drops unusable stages and rejects a session with nothing left to run.
    pub fn sanitised(mut self) -> Result<Self, String> {
        if self.name.trim().is_empty() {
            return Err("The session needs a name.".to_string());
        }
        self.stages.retain(SessionStage::is_usable);
        if self.stages.is_empty() {
            return Err("A session needs at least one stage with a name and a duration.".to_string());
        }
        Ok(self)
    }
}

/// Coarse state of the session as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// No session is loaded.
    Idle,
    /// A stage is counting down.
    Running,
    /// A stage finished and the session is between stages.
    Awaiting,
    /// Every stage finished.
    Completed,
}

/// Broadcast description of the session cursor.
///
/// Both windows render this to identify the current stage; it carries no
/// countdown of its own, because the remaining time is the timer snapshot's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session_id: Option<String>,
    pub session_name: String,
    pub status: SessionStatus,
    /// Zero-based index of the current stage.
    pub stage_index: usize,
    pub stage_count: usize,
    pub stage_name: String,
    /// True while the operator must trigger the next stage.
    pub awaiting_manual: bool,
    /// Remaining time of an automatic transition countdown, if one is running.
    pub transition_countdown_ms: Option<i64>,
    /// The message the current stage shows, if any.
    pub active_message: Option<StageMessage>,
    pub updated_at: i64,
}

impl SessionSnapshot {
    /// Snapshot describing "no session", used before anything is started.
    pub fn idle(now_ms: i64) -> Self {
        Self {
            session_id: None,
            session_name: String::new(),
            status: SessionStatus::Idle,
            stage_index: 0,
            stage_count: 0,
            stage_name: String::new(),
            awaiting_manual: false,
            transition_countdown_ms: None,
            active_message: None,
            updated_at: now_ms,
        }
    }

    /// True when a session is actually loaded.
    pub fn is_active(&self) -> bool {
        self.session_id.is_some()
    }
}

/// Serialisable form of the cursor, written to disk so a session survives a
/// restart. The stage *countdown* is not stored here — the timer engine already
/// persists the running stage, so this record only has to say which stage it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub definition: SessionDefinition,
    pub stage_index: usize,
    pub status: SessionStatus,
    pub awaiting_manual: bool,
    /// Wall-clock deadline of an automatic transition countdown.
    #[serde(default)]
    pub transition_deadline_at: Option<i64>,
    /// Warning thresholds already announced for the current stage.
    #[serde(default)]
    pub fired_warnings_ms: Vec<i64>,
    pub updated_at: i64,
}

/// The session cursor: which stage is current and what happens next.
#[derive(Debug, Clone)]
pub struct SessionMachine {
    definition: SessionDefinition,
    stage_index: usize,
    status: SessionStatus,
    awaiting_manual: bool,
    /// Wall-clock deadline for an automatic inter-stage transition.
    transition_deadline_at: Option<i64>,
    fired_warnings_ms: Vec<i64>,
    updated_at: i64,
}

/// What the engine should do after a stage's timer reached zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    /// Start the next stage immediately.
    Advance,
    /// Wait for the operator before starting the next stage.
    AwaitOperator,
    /// Wait `countdown` milliseconds (which may be zero) and then advance.
    AwaitCountdown(i64),
    /// There is no next stage; the session is finished.
    Finished,
}

impl SessionMachine {
    /// A cursor with no session loaded.
    pub fn empty(now_ms: i64) -> Self {
        Self {
            definition: SessionDefinition {
                id: String::new(),
                name: String::new(),
                stages: Vec::new(),
            },
            stage_index: 0,
            status: SessionStatus::Idle,
            awaiting_manual: false,
            transition_deadline_at: None,
            fired_warnings_ms: Vec::new(),
            updated_at: now_ms,
        }
    }

    /// Loads a definition and points at its first stage.
    pub fn start(definition: SessionDefinition, wall_now: i64) -> Self {
        Self {
            definition,
            stage_index: 0,
            status: SessionStatus::Running,
            awaiting_manual: false,
            transition_deadline_at: None,
            fired_warnings_ms: Vec::new(),
            updated_at: wall_now,
        }
    }

    /// Rebuilds the cursor from a persisted record.
    pub fn restore(record: SessionRecord, wall_now: i64) -> Self {
        if record.definition.stages.is_empty() {
            return Self::empty(wall_now);
        }
        let last = record.definition.stages.len() - 1;
        Self {
            definition: record.definition,
            stage_index: record.stage_index.min(last),
            status: record.status,
            awaiting_manual: record.awaiting_manual,
            transition_deadline_at: record.transition_deadline_at,
            fired_warnings_ms: record.fired_warnings_ms,
            updated_at: record.updated_at,
        }
    }

    pub fn status(&self) -> SessionStatus {
        self.status
    }

    pub fn stage_index(&self) -> usize {
        self.stage_index
    }

    pub fn stage_count(&self) -> usize {
        self.definition.stages.len()
    }

    pub fn is_loaded(&self) -> bool {
        self.status != SessionStatus::Idle && !self.definition.stages.is_empty()
    }

    pub fn current_stage(&self) -> Option<&SessionStage> {
        self.definition.stages.get(self.stage_index)
    }

    /// Remaining time of a running automatic transition countdown.
    pub fn transition_remaining_ms(&self, wall_now: i64) -> Option<i64> {
        self.transition_deadline_at
            .map(|deadline| (deadline - wall_now).max(0))
    }

    /// True when an automatic transition countdown has elapsed.
    pub fn transition_due(&self, wall_now: i64) -> bool {
        self.status == SessionStatus::Awaiting
            && !self.awaiting_manual
            && self
                .transition_deadline_at
                .is_some_and(|deadline| deadline <= wall_now)
    }

    /// Warning thresholds of the current stage that have not fired yet.
    pub fn pending_warnings(&self, remaining_ms: i64, duration_ms: i64) -> Vec<i64> {
        let Some(stage) = self.current_stage() else {
            return Vec::new();
        };
        stage
            .warning_intervals_ms
            .iter()
            .copied()
            .filter(|threshold| {
                *threshold > 0
                    && *threshold < duration_ms
                    && remaining_ms <= *threshold
                    && !self.fired_warnings_ms.contains(threshold)
            })
            .collect()
    }

    /// Records that a warning threshold has been announced.
    pub fn mark_warning_fired(&mut self, threshold_ms: i64) {
        if !self.fired_warnings_ms.contains(&threshold_ms) {
            self.fired_warnings_ms.push(threshold_ms);
        }
    }

    /// Decides what happens now that the current stage's timer reached zero.
    pub fn complete_stage(&mut self, wall_now: i64) -> StageOutcome {
        let Some(stage) = self.current_stage() else {
            self.status = SessionStatus::Completed;
            self.updated_at = wall_now;
            return StageOutcome::Finished;
        };

        let is_last = self.stage_index + 1 >= self.definition.stages.len();
        if is_last {
            self.status = SessionStatus::Completed;
            self.awaiting_manual = false;
            self.transition_deadline_at = None;
            self.updated_at = wall_now;
            return StageOutcome::Finished;
        }

        match stage.transition {
            StageTransition::Manual => {
                self.status = SessionStatus::Awaiting;
                self.awaiting_manual = true;
                self.transition_deadline_at = None;
                self.updated_at = wall_now;
                StageOutcome::AwaitOperator
            }
            StageTransition::Auto => {
                let countdown = stage.transition_countdown_ms.unwrap_or(0).max(0);
                if countdown == 0 {
                    self.status = SessionStatus::Awaiting;
                    self.awaiting_manual = false;
                    self.transition_deadline_at = Some(wall_now);
                    self.updated_at = wall_now;
                    StageOutcome::Advance
                } else {
                    self.status = SessionStatus::Awaiting;
                    self.awaiting_manual = false;
                    self.transition_deadline_at = Some(wall_now + countdown);
                    self.updated_at = wall_now;
                    StageOutcome::AwaitCountdown(countdown)
                }
            }
        }
    }

    /// Moves the cursor to the next stage and marks it running.
    ///
    /// Returns `false` when there is no next stage, leaving the session
    /// completed.
    pub fn advance(&mut self, wall_now: i64) -> bool {
        if self.stage_index + 1 >= self.definition.stages.len() {
            self.status = SessionStatus::Completed;
            self.awaiting_manual = false;
            self.transition_deadline_at = None;
            self.updated_at = wall_now;
            return false;
        }

        self.stage_index += 1;
        self.status = SessionStatus::Running;
        self.awaiting_manual = false;
        self.transition_deadline_at = None;
        self.fired_warnings_ms.clear();
        self.updated_at = wall_now;
        true
    }

    /// Clears the session entirely.
    pub fn clear(&mut self, wall_now: i64) {
        *self = Self::empty(wall_now);
    }

    /// Broadcast description of the current cursor position.
    pub fn snapshot(&self, wall_now: i64) -> SessionSnapshot {
        if !self.is_loaded() {
            return SessionSnapshot::idle(wall_now);
        }

        let stage = self.current_stage();
        SessionSnapshot {
            session_id: Some(self.definition.id.clone()),
            session_name: self.definition.name.clone(),
            status: self.status,
            stage_index: self.stage_index,
            stage_count: self.definition.stages.len(),
            stage_name: stage.map(|value| value.name.clone()).unwrap_or_default(),
            awaiting_manual: self.awaiting_manual,
            transition_countdown_ms: self.transition_remaining_ms(wall_now),
            active_message: stage.and_then(|value| value.message.clone()),
            updated_at: self.updated_at,
        }
    }

    /// Serialisable form of the cursor.
    pub fn to_record(&self) -> SessionRecord {
        SessionRecord {
            definition: self.definition.clone(),
            stage_index: self.stage_index,
            status: self.status,
            awaiting_manual: self.awaiting_manual,
            transition_deadline_at: self.transition_deadline_at,
            fired_warnings_ms: self.fired_warnings_ms.clone(),
            updated_at: self.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: i64 = 1_000;

    fn stage(name: &str, seconds: i64, transition: StageTransition) -> SessionStage {
        SessionStage {
            id: format!("stage-{name}"),
            name: name.to_string(),
            duration_ms: seconds * SECOND,
            message: None,
            warning_intervals_ms: Vec::new(),
            sound_enabled: false,
            transition,
            transition_countdown_ms: None,
        }
    }

    fn session(stages: Vec<SessionStage>) -> SessionDefinition {
        SessionDefinition {
            id: "session-1".to_string(),
            name: "Examination".to_string(),
            stages,
        }
    }

    #[test]
    fn starting_a_session_points_at_the_first_stage() {
        let machine = SessionMachine::start(
            session(vec![
                stage("Reading", 60, StageTransition::Auto),
                stage("Writing", 90, StageTransition::Manual),
            ]),
            10,
        );

        assert_eq!(machine.status(), SessionStatus::Running);
        assert_eq!(machine.stage_index(), 0);
        assert_eq!(machine.stage_count(), 2);
        assert_eq!(machine.current_stage().unwrap().name, "Reading");
    }

    #[test]
    fn an_auto_stage_advances_straight_to_the_next() {
        let mut machine = SessionMachine::start(
            session(vec![
                stage("Reading", 60, StageTransition::Auto),
                stage("Writing", 90, StageTransition::Auto),
            ]),
            0,
        );

        assert_eq!(machine.complete_stage(SECOND), StageOutcome::Advance);
        assert!(machine.advance(2 * SECOND));
        assert_eq!(machine.stage_index(), 1);
        assert_eq!(machine.current_stage().unwrap().name, "Writing");
        assert_eq!(machine.status(), SessionStatus::Running);
    }

    #[test]
    fn a_manual_stage_waits_for_the_operator() {
        let mut machine = SessionMachine::start(
            session(vec![
                stage("Reading", 60, StageTransition::Manual),
                stage("Writing", 90, StageTransition::Auto),
            ]),
            0,
        );

        assert_eq!(machine.complete_stage(SECOND), StageOutcome::AwaitOperator);
        assert_eq!(machine.status(), SessionStatus::Awaiting);
        assert!(machine.snapshot(SECOND).awaiting_manual);
    }

    #[test]
    fn a_transition_countdown_is_reported_until_it_elapses() {
        let mut stages = vec![
            stage("Reading", 60, StageTransition::Auto),
            stage("Writing", 90, StageTransition::Auto),
        ];
        stages[0].transition_countdown_ms = Some(5 * SECOND);

        let mut machine = SessionMachine::start(session(stages), 1_000);
        assert_eq!(machine.complete_stage(1_000), StageOutcome::AwaitCountdown(5 * SECOND));

        assert_eq!(machine.transition_remaining_ms(1_000), Some(5 * SECOND));
        assert_eq!(machine.transition_remaining_ms(3_000), Some(3 * SECOND));
        assert!(!machine.transition_due(5_999));
        assert!(machine.transition_due(6_000));
    }

    #[test]
    fn the_last_stage_completes_the_session() {
        let mut machine = SessionMachine::start(
            session(vec![stage("Only", 30, StageTransition::Auto)]),
            0,
        );

        assert_eq!(machine.complete_stage(SECOND), StageOutcome::Finished);
        assert_eq!(machine.status(), SessionStatus::Completed);
        assert!(!machine.advance(2 * SECOND));
        assert_eq!(machine.status(), SessionStatus::Completed);
    }

    #[test]
    fn warnings_fire_once_each_at_their_threshold() {
        let mut stages = vec![stage("Reading", 60, StageTransition::Auto)];
        stages[0].warning_intervals_ms = vec![30 * SECOND, 10 * SECOND];
        let mut machine = SessionMachine::start(session(stages), 0);

        // 61s remaining: nothing has crossed yet.
        assert!(machine.pending_warnings(61 * SECOND, 60 * SECOND).is_empty());

        // 30s remaining: the first threshold has been crossed.
        let fired = machine.pending_warnings(30 * SECOND, 60 * SECOND);
        assert_eq!(fired, vec![30 * SECOND]);
        machine.mark_warning_fired(30 * SECOND);
        assert!(machine.pending_warnings(29 * SECOND, 60 * SECOND).is_empty());

        // 10s remaining: the second threshold is crossed, not the first again.
        let fired = machine.pending_warnings(10 * SECOND, 60 * SECOND);
        assert_eq!(fired, vec![10 * SECOND]);
    }

    #[test]
    fn a_threshold_longer_than_the_stage_never_fires() {
        let mut stages = vec![stage("Short", 30, StageTransition::Auto)];
        stages[0].warning_intervals_ms = vec![60 * SECOND];
        let machine = SessionMachine::start(session(stages), 0);

        assert!(machine.pending_warnings(30 * SECOND, 30 * SECOND).is_empty());
        assert!(machine.pending_warnings(0, 30 * SECOND).is_empty());
    }

    #[test]
    fn advancing_clears_the_previous_stage_warnings() {
        let mut stages = vec![
            stage("Reading", 60, StageTransition::Auto),
            stage("Writing", 90, StageTransition::Auto),
        ];
        stages[0].warning_intervals_ms = vec![30 * SECOND];
        let mut machine = SessionMachine::start(session(stages), 0);

        machine.mark_warning_fired(30 * SECOND);
        machine.complete_stage(SECOND);
        assert!(machine.advance(SECOND));

        // The new stage has a clean slate.
        assert!(machine.pending_warnings(29 * SECOND, 90 * SECOND).is_empty());
    }

    #[test]
    fn a_round_trip_through_a_record_preserves_the_cursor() {
        let mut stages = vec![
            stage("Reading", 60, StageTransition::Manual),
            stage("Writing", 90, StageTransition::Auto),
        ];
        stages[0].transition_countdown_ms = Some(4 * SECOND);
        let mut machine = SessionMachine::start(session(stages), 0);
        machine.complete_stage(1_000);

        let record = machine.to_record();
        let restored = SessionMachine::restore(record, 2_000);

        assert_eq!(restored.stage_index(), 0);
        assert_eq!(restored.status(), SessionStatus::Awaiting);
        assert!(restored.snapshot(2_000).awaiting_manual);
    }

    #[test]
    fn a_record_without_stages_is_discarded() {
        let record = SessionRecord {
            definition: SessionDefinition {
                id: "session-1".to_string(),
                name: "Empty".to_string(),
                stages: Vec::new(),
            },
            stage_index: 0,
            status: SessionStatus::Running,
            awaiting_manual: false,
            transition_deadline_at: None,
            fired_warnings_ms: Vec::new(),
            updated_at: 0,
        };

        let machine = SessionMachine::restore(record, 5);
        assert_eq!(machine.status(), SessionStatus::Idle);
        assert!(!machine.is_loaded());
    }

    #[test]
    fn unusable_stages_are_dropped_and_an_empty_session_is_rejected() {
        let mut broken = stage("Broken", 0, StageTransition::Auto);
        broken.duration_ms = 0;
        let mut unnamed = stage("Unnamed", 30, StageTransition::Auto);
        unnamed.name = "  ".to_string();

        let definition = session(vec![
            stage("Reading", 60, StageTransition::Auto),
            broken,
            unnamed,
        ]);

        let sanitised = definition.sanitised().expect("one usable stage remains");
        assert_eq!(sanitised.stages.len(), 1);

        let empty = session(vec![SessionStage {
            duration_ms: 0,
            ..stage("Nothing", 0, StageTransition::Auto)
        }]);
        assert!(empty.sanitised().is_err());
    }
}
