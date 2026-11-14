import type { MessageAnimation, MessageFont, MessagePosition } from "./message";

/**
 * The resolved message a stage shows, field-for-field mirror of `StageMessage`
 * in `src-tauri/src/session/state.rs`. The control window resolves a message
 * *id* into this payload when a session starts, so Rust can present it on its
 * own without the display window waiting for a forward.
 */
export interface StageMessage {
  id: string;
  name: string;
  text: string;
  fontSize: number;
  color: string;
  backgroundColor: string | null;
  position: MessagePosition | string;
  font: MessageFont | string;
  animation: MessageAnimation | string;
  /** How long the message stays on screen, in milliseconds; `0` is sticky. */
  durationMs: number;
}

/** How a session moves from one stage to the next. */
export type StageTransition = "auto" | "manual";

export interface SessionStage {
  id: string;
  name: string;
  durationMs: number;
  /** Message shown while this stage runs, if any. */
  message: StageMessage | null;
  /** Remaining-time thresholds (ms) at which a warning is announced. */
  warningIntervalsMs: number[];
  /** Whether a sound plays when the stage starts and its warnings fire. */
  soundEnabled: boolean;
  transition: StageTransition;
  /** Optional pause between this stage and the next, in milliseconds. */
  transitionCountdownMs: number | null;
}

/**
 * A sequential set of timer stages. Mirrors `SessionDefinition` in Rust; the
 * definition is stored in the library, the running cursor is the engine's.
 */
export interface SessionDefinition {
  id: string;
  name: string;
  stages: SessionStage[];
}

export type SessionStatus = "idle" | "running" | "awaiting" | "completed";

/**
 * Field-for-field mirror of `SessionSnapshot` in `src-tauri/src/session/state.rs`.
 *
 * It carries a stage description but no countdown: the remaining time is always
 * the timer snapshot's, so the two engines can never disagree.
 */
export interface SessionSnapshot {
  sessionId: string | null;
  sessionName: string;
  status: SessionStatus;
  stageIndex: number;
  stageCount: number;
  stageName: string;
  awaitingManual: boolean;
  transitionCountdownMs: number | null;
  activeMessage: StageMessage | null;
  updatedAt: number;
}

export const IDLE_SESSION: SessionSnapshot = {
  sessionId: null,
  sessionName: "",
  status: "idle",
  stageIndex: 0,
  stageCount: 0,
  stageName: "",
  awaitingManual: false,
  transitionCountdownMs: null,
  activeMessage: null,
  updatedAt: 0,
};

/** A discrete session moment the frontend reacts to (sound, notifications). */
export type SessionCueKind =
  | "stage-start"
  | "warning"
  | "stage-complete"
  | "session-complete";

export interface SessionCue {
  kind: SessionCueKind;
  sessionId: string;
  sessionName: string;
  stageIndex: number;
  stageCount: number;
  stageName: string;
  /** Remaining milliseconds when the cue fired, for warnings. */
  remainingMs: number | null;
  /** Whether the operator asked for a sound at this moment. */
  sound: boolean;
}
