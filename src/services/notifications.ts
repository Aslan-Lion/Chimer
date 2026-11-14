import { playSound } from "../lib/sound";
import { isControlWindow } from "./backend";
import { sessionStore } from "../stores/sessionStore";
import { settingsStore } from "../stores/settingsStore";
import { timerStore } from "../stores/timerStore";
import type { TimerStatus } from "../types";

/**
 * Sound notification engine.
 *
 * Every cue is derived from authoritative state, never from a frontend clock:
 * the timer engine's snapshot drives completion, and the session engine's cue
 * channel drives warnings, stage starts and session end. Only the *choice* of
 * sound is a frontend concern, and it is read from the operator's notification
 * settings at the moment the cue arrives.
 *
 * The module deliberately owns no audio machinery of its own — it delegates to
 * `lib/sound`, which synthesises the tones — and it never creates event
 * channels: it consumes the two bridges the application already runs.
 */

/** True while a session owns the countdown, whatever stage it is on. */
function sessionActive(): boolean {
  const snapshot = sessionStore.get().snapshot;
  return snapshot !== null && snapshot.sessionId !== null;
}

/**
 * Plays the sound for a completed countdown.
 *
 * A stage completion also completes a timer, so the session engine reports it
 * through its own cue; this path is reserved for a plain countdown, which is
 * what stops a session stage and a session end from producing two sounds.
 */
function handleTimerCompletion(): void {
  if (sessionActive()) return;

  const { notifications } = settingsStore.get().settings;
  if (!notifications.soundsEnabled) return;
  playSound(notifications.completionSound);
}

/**
 * Watches the timer snapshot and plays the completion sound on the transition.
 *
 * The previous status is seeded from the snapshot already in the store, so a
 * countdown that is *already* complete when the application starts does not
 * announce itself again — a restart never replays a notification. Statuses are
 * tracked on every update, including the ones where nothing is played, so a
 * suppressed cue cannot fire later on an unrelated change.
 */
export function startNotificationBridge(): () => void {
  if (!isControlWindow()) return () => undefined;

  let previous: TimerStatus | null = timerStore.get().snapshot?.status ?? null;

  return timerStore.subscribe(() => {
    const status = timerStore.get().snapshot?.status ?? null;
    const completed = status === "completed" && previous !== "completed";
    previous = status;
    if (completed) handleTimerCompletion();
  });
}
