export type TimerStatus = "idle" | "running" | "paused" | "completed" | "stopped";

/**
 * A timer definition owned by the library. The authoritative runtime values
 * (remaining time, status, overtime) live in the Rust engine and are delivered
 * as a {@link TimerSnapshot}.
 */
export interface TimerDefinition {
  id: string;
  name: string;
  durationMs: number;
  messageId: string | null;
  soundEnabled: boolean;
  createdAt: number;
}

/**
 * Field-for-field mirror of `TimerSnapshot` in `src-tauri/src/timer/state.rs`.
 *
 * This is the *only* timer value either window renders: Rust computes the
 * countdown, the progress and the overtime counter and broadcasts this payload
 * over `timer://state-changed`. The frontend never counts anything down itself,
 * so the two windows can never disagree.
 */
export interface TimerSnapshot {
  /** `null` when no timer has been configured yet. */
  timerId: string | null;
  name: string;
  status: TimerStatus;
  /** The duration the timer was configured with. */
  durationMs: number;
  /** Live remaining time, clamped to zero. Never negative. */
  remainingMs: number;
  /** Fraction of the duration already elapsed, 0.0 – 1.0. */
  progress: number;
  /** Time elapsed since completion; zero while the timer has not completed. */
  overtimeMs: number;
  startedAt: number | null;
  completedAt: number | null;
  /** Monotonically increasing revision, useful for diagnostics. */
  revision: number;
  updatedAt: number;
}
