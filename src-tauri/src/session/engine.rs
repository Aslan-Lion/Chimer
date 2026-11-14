//! The authoritative session engine.
//!
//! One [`SessionEngine`] instance is created per process and managed by Tauri,
//! exactly like the timer engine. It owns the *session cursor* and delegates
//! every countdown to [`crate::timer`]: starting a stage means configuring and
//! starting an ordinary timer, so there is still only one timer engine in the
//! application.
//!
//! The engine is driven by the same background heartbeat as the timer. On each
//! tick it
//!
//! * announces warning thresholds the running stage has crossed,
//! * notices that the stage's timer reached zero and decides what comes next
//!   (advance immediately, wait for the operator, or wait out a transition
//!   countdown),
//! * writes the resulting cursor through to [`super::persistence`],
//! * broadcasts the cursor and pushes the stage's message to the display
//!   window.
//!
//! It never trusts its own clock: the stage's remaining time is always the
//! timer snapshot's, so the two engines can never disagree.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager};

use crate::display::{DisplayMessage, SessionPresentation};
use crate::timer::engine::{wall_now_ms, TimerEngine};
use crate::timer::state::TimerStatus;

use super::events::{self, SessionCue};
use super::persistence;
use super::state::{
    SessionDefinition, SessionMachine, SessionSnapshot, SessionStatus, StageOutcome,
};

/// A discrete step the engine applies after inspecting the cursor.
enum Step {
    /// A warning threshold was crossed.
    Warning { remaining_ms: i64, sound: bool },
    /// The running stage's timer reached zero.
    StageComplete { sound: bool },
    /// The last stage finished.
    SessionComplete { sound: bool },
}

/// Authoritative owner of the single active session.
pub struct SessionEngine {
    machine: Mutex<SessionMachine>,
    store: Mutex<Option<PathBuf>>,
    /// Signature of the last broadcast cursor, used to drop duplicate events.
    last_signature: Mutex<Option<String>>,
}

impl Default for SessionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionEngine {
    pub fn new() -> Self {
        Self {
            machine: Mutex::new(SessionMachine::empty(wall_now_ms())),
            store: Mutex::new(None),
            last_signature: Mutex::new(None),
        }
    }

    /// Restores the persisted cursor and remembers where future changes go.
    ///
    /// The stage countdown is *not* recreated here: the timer engine restores
    /// the running stage's remaining time from its own document during its own
    /// startup, so re-arming it would silently reset the stage.
    pub fn initialize(&self, app: &AppHandle) {
        let Some(path) = persistence::resolve_path(app) else {
            return;
        };

        if let Some(record) = persistence::load(&path) {
            *self.machine() = SessionMachine::restore(record, wall_now_ms());
        }

        *self.store() = Some(path);

        let snapshot = self.machine().snapshot(wall_now_ms());
        if snapshot.is_active() {
            self.broadcast(app, &snapshot);
            self.push_presentation(app, &snapshot);
        }
    }

    // ------------------------------------------------------------- commands

    /// Loads a session and starts its first stage.
    pub fn start(
        &self,
        app: &AppHandle,
        definition: SessionDefinition,
    ) -> Result<SessionSnapshot, String> {
        let definition = definition.sanitised()?;
        let wall = wall_now_ms();

        let stage = definition
            .stages
            .first()
            .cloned()
            .ok_or_else(|| "A session needs at least one stage.".to_string())?;

        *self.machine() = SessionMachine::start(definition, wall);

        // The stage is an ordinary timer: the timer engine remains the single
        // authority for the countdown.
        self.arm_stage(app, &stage.name, stage.duration_ms)?;

        let snapshot = self.machine().snapshot(wall);
        self.persist();
        self.broadcast(app, &snapshot);
        self.push_presentation(app, &snapshot);
        self.cue(app, &snapshot, "stage-start", None, stage.sound_enabled);

        Ok(snapshot)
    }

    /// Moves to the next stage: the operator's action at a manual transition,
    /// or a deliberate skip of a transition countdown.
    pub fn next(&self, app: &AppHandle) -> Result<SessionSnapshot, String> {
        let wall = wall_now_ms();

        let (advanced, snapshot, sound, next_stage) = {
            let mut machine = self.machine();
            if !machine.is_loaded() {
                return Err("No session is running.".to_string());
            }
            if machine.status() == SessionStatus::Completed {
                return Err("The session has already finished.".to_string());
            }

            let sound = machine
                .current_stage()
                .map(|stage| stage.sound_enabled)
                .unwrap_or(false);

            let advanced = machine.advance(wall);
            // The duration has to come from the machine: the snapshot only
            // names the stage, it never carries its length.
            let next_stage = machine
                .current_stage()
                .map(|stage| (stage.name.clone(), stage.duration_ms));
            let snapshot = machine.snapshot(wall);
            (advanced, snapshot, sound, next_stage)
        };

        if advanced {
            if let Some((name, duration_ms)) = next_stage {
                self.arm_stage(app, &name, duration_ms)?;
            }
            self.persist();
            self.broadcast(app, &snapshot);
            self.push_presentation(app, &snapshot);
            self.cue(app, &snapshot, "stage-start", None, sound);
        } else {
            // There was nothing left: the session is complete.
            self.persist();
            self.broadcast(app, &snapshot);
            self.push_presentation(app, &snapshot);
            self.cue(app, &snapshot, "session-complete", None, sound);
        }

        Ok(snapshot)
    }

    /// Ends the session and returns both windows to the idle presentation.
    pub fn stop(&self, app: &AppHandle) -> Result<SessionSnapshot, String> {
        let wall = wall_now_ms();

        {
            let mut machine = self.machine();
            machine.clear(wall);
        }

        // A session that is stopped must not leave a stray countdown behind.
        if let Some(timer) = app.try_state::<TimerEngine>() {
            let _ = timer.abort(app);
        }

        self.clear_persisted();
        let snapshot = self.machine().snapshot(wall);
        self.broadcast(app, &snapshot);
        self.push_presentation(app, &snapshot);

        Ok(snapshot)
    }

    /// Current cursor, without broadcasting.
    pub fn current(&self) -> SessionSnapshot {
        self.machine().snapshot(wall_now_ms())
    }

    /// Background heartbeat, run alongside the timer's own tick.
    pub fn tick(&self, app: &AppHandle) {
        let Some(timer) = app.try_state::<TimerEngine>() else {
            return;
        };

        let wall = wall_now_ms();
        let timer_snapshot = timer.current();

        let mut steps: Vec<Step> = Vec::new();
        let mut advance_to: Option<(String, i64, bool)> = None;
        let mut dirty = false;

        let snapshot = {
            let mut machine = self.machine();
            if !machine.is_loaded() {
                return;
            }

            match machine.status() {
                SessionStatus::Running => {
                    let stage = machine.current_stage().cloned();
                    let (duration, sound) = stage
                        .as_ref()
                        .map(|value| (value.duration_ms, value.sound_enabled))
                        .unwrap_or((0, false));

                    let pending = machine.pending_warnings(timer_snapshot.remaining_ms, duration);
                    if !pending.is_empty() {
                        for threshold in pending {
                            machine.mark_warning_fired(threshold);
                        }
                        dirty = true;
                        steps.push(Step::Warning {
                            remaining_ms: timer_snapshot.remaining_ms,
                            sound,
                        });
                    }

                    if timer_snapshot.status == TimerStatus::Completed {
                        steps.push(Step::StageComplete { sound });
                        match machine.complete_stage(wall) {
                            StageOutcome::Finished => {
                                steps.push(Step::SessionComplete { sound });
                                dirty = true;
                            }
                            StageOutcome::AwaitOperator | StageOutcome::AwaitCountdown(_) => {
                                dirty = true;
                            }
                            StageOutcome::Advance => {
                                dirty = true;
                                if machine.advance(wall) {
                                    advance_to = machine.current_stage().map(|next| {
                                        (next.name.clone(), next.duration_ms, next.sound_enabled)
                                    });
                                }
                            }
                        }
                    }
                }
                SessionStatus::Awaiting => {
                    if machine.transition_due(wall) {
                        if machine.advance(wall) {
                            dirty = true;
                            advance_to = machine.current_stage().map(|next| {
                                (next.name.clone(), next.duration_ms, next.sound_enabled)
                            });
                        }
                    }
                }
                _ => {}
            }

            let snapshot = machine.snapshot(wall);
            if dirty {
                self.persist_record(&machine.to_record());
            }
            snapshot
        };

        if let Some((name, duration, _sound)) = advance_to.as_ref() {
            // Outside the cursor lock, so the timer engine is never asked to do
            // work while the session lock is held.
            let _ = self.arm_stage(app, name, *duration);
        }

        for step in &steps {
            match step {
                Step::Warning {
                    remaining_ms,
                    sound,
                } => {
                    self.cue(app, &snapshot, "warning", Some(*remaining_ms), *sound);
                }
                Step::StageComplete { sound } => {
                    self.cue(app, &snapshot, "stage-complete", None, *sound);
                }
                Step::SessionComplete { sound } => {
                    self.cue(app, &snapshot, "session-complete", None, *sound);
                }
            }
        }

        if let Some((_, _, sound)) = advance_to {
            self.cue(app, &snapshot, "stage-start", None, sound);
        }

        self.broadcast_if_changed(app, &snapshot);
        self.push_presentation(app, &snapshot);
    }

    // ------------------------------------------------------------ internals

    /// Configures and starts the timer for a stage.
    fn arm_stage(&self, app: &AppHandle, name: &str, duration_ms: i64) -> Result<(), String> {
        let Some(timer) = app.try_state::<TimerEngine>() else {
            return Err("The timer engine is not available.".to_string());
        };
        timer.create(app, name.to_string(), duration_ms)?;
        timer.start(app)?;
        Ok(())
    }

    fn persist(&self) {
        let record = self.machine().to_record();
        self.persist_record(&record);
    }

    fn persist_record(&self, record: &super::state::SessionRecord) {
        if let Some(path) = self.store().clone() {
            persistence::save(&path, record);
        }
    }

    fn clear_persisted(&self) {
        if let Some(path) = self.store().clone() {
            persistence::clear(&path);
        }
    }

    /// Emits the cursor when it actually changed.
    fn broadcast_if_changed(&self, app: &AppHandle, snapshot: &SessionSnapshot) {
        let signature = Self::signature(snapshot);
        {
            let mut last = self.last_signature();
            if last.as_deref() == Some(signature.as_str()) {
                return;
            }
            *last = Some(signature);
        }
        events::emit_state(app, snapshot);
    }

    /// Emits the cursor unconditionally.
    fn broadcast(&self, app: &AppHandle, snapshot: &SessionSnapshot) {
        *self.last_signature() = Some(Self::signature(snapshot));
        events::emit_state(app, snapshot);
    }

    fn signature(snapshot: &SessionSnapshot) -> String {
        format!(
            "{:?}|{}|{}|{}|{}",
            snapshot.status,
            snapshot.stage_index,
            snapshot.awaiting_manual,
            snapshot
                .transition_countdown_ms
                .map(|value| value / 1_000)
                .unwrap_or(-1),
            snapshot.session_id.as_deref().unwrap_or("-"),
        )
    }

    /// Pushes the stage description (and its message) to the display window.
    fn push_presentation(&self, app: &AppHandle, snapshot: &SessionSnapshot) {
        if !snapshot.is_active() {
            crate::display::sync_session(app, None);
            return;
        }

        let message = snapshot.active_message.as_ref().map(|value| DisplayMessage {
            id: value.id.clone(),
            name: value.name.clone(),
            text: value.text.clone(),
            font_size: value.font_size,
            color: value.color.clone(),
            background_color: value.background_color.clone(),
            position: value.position.clone(),
            font: value.font.clone(),
            animation: value.animation.clone(),
            duration_ms: value.duration_ms,
        });

        let presentation = SessionPresentation {
            session_id: snapshot.session_id.clone().unwrap_or_default(),
            session_name: snapshot.session_name.clone(),
            stage_index: snapshot.stage_index,
            stage_count: snapshot.stage_count,
            stage_name: snapshot.stage_name.clone(),
            awaiting_manual: snapshot.awaiting_manual,
            transition_countdown_ms: snapshot.transition_countdown_ms,
            message,
        };

        crate::display::sync_session(app, Some(presentation));
    }

    fn cue(
        &self,
        app: &AppHandle,
        snapshot: &SessionSnapshot,
        kind: &'static str,
        remaining_ms: Option<i64>,
        sound: bool,
    ) {
        events::emit_cue(
            app,
            &SessionCue {
                kind,
                session_id: snapshot.session_id.clone().unwrap_or_default(),
                session_name: snapshot.session_name.clone(),
                stage_index: snapshot.stage_index,
                stage_count: snapshot.stage_count,
                stage_name: snapshot.stage_name.clone(),
                remaining_ms,
                sound,
            },
        );
    }

    /// Locking helpers that survive a poisoned mutex.
    fn machine(&self) -> MutexGuard<'_, SessionMachine> {
        self.machine.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn store(&self) -> MutexGuard<'_, Option<PathBuf>> {
        self.store.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn last_signature(&self) -> MutexGuard<'_, Option<String>> {
        self.last_signature
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
