import type { TimerSnapshot } from "../types";
import { createStore } from "./createStore";
import {
  abortTimer as abortRemoteTimer,
  createTimer as createRemoteTimer,
  getTimerSnapshot,
  onTimerState,
  pauseTimer as pauseRemoteTimer,
  resumeTimer as resumeRemoteTimer,
  startTimer as startRemoteTimer,
  stopTimer as stopRemoteTimer,
} from "../services/timer";

interface TimerStoreState {
  /**
   * Last snapshot broadcast by the Rust engine, or `null` before the first one
   * arrives. A snapshot whose `timerId` is `null` means no timer is configured.
   * The store holds no clock of its own: it is a pure mirror, never a counter.
   */
  snapshot: TimerSnapshot | null;
  /** True while a command is in flight, so the controls can be held. */
  busy: boolean;
  error: string | null;
}

export const timerStore = createStore<TimerStoreState>({
  snapshot: null,
  busy: false,
  error: null,
});

function describe(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** Applies an authoritative snapshot. Both windows receive it through this path. */
export function applyTimerSnapshot(snapshot: TimerSnapshot): void {
  timerStore.set({ snapshot });
}

/**
 * Takes ownership of the pending error message. Callers that display the
 * failure themselves use this so it is reported once, in the right place.
 */
export function clearTimerError(): string | null {
  const { error } = timerStore.get();
  if (error !== null) timerStore.set({ error: null });
  return error;
}

/** Reads the current snapshot; used on startup and after the bridge is attached. */
export async function loadTimerSnapshot(): Promise<void> {
  try {
    const snapshot = await getTimerSnapshot();
    if (snapshot) applyTimerSnapshot(snapshot);
  } catch (cause) {
    timerStore.set({ error: describe(cause) });
  }
}

/**
 * Subscribes to the Rust broadcasts and loads the current snapshot.
 *
 * The listener is registered *before* the initial read, so an update that
 * happens in between cannot be missed. The returned function detaches it.
 */
export async function startTimerBridge(): Promise<() => void> {
  const unlisten = await onTimerState(applyTimerSnapshot);
  await loadTimerSnapshot();
  return unlisten;
}

/**
 * Runs one engine command. The engine broadcasts the resulting state to both
 * windows itself; the value it returns is applied here as well so the calling
 * window reacts without waiting for the round trip.
 */
async function run(command: () => Promise<TimerSnapshot>): Promise<boolean> {
  timerStore.set({ busy: true, error: null });
  try {
    applyTimerSnapshot(await command());
    timerStore.set({ busy: false });
    return true;
  } catch (cause) {
    timerStore.set({ busy: false, error: describe(cause) });
    return false;
  }
}

/**
 * Configures a timer in the engine and starts it immediately.
 *
 * This is what every "start this countdown" affordance in the interface calls:
 * the definition is validated by Rust, which also owns the resulting state.
 */
export async function startNewTimer(name: string, durationMs: number): Promise<boolean> {
  timerStore.set({ busy: true, error: null });
  try {
    await createRemoteTimer(name, durationMs);
  } catch (cause) {
    timerStore.set({ busy: false, error: describe(cause) });
    return false;
  }
  return run(startRemoteTimer);
}

export function startActiveTimer(): Promise<boolean> {
  return run(startRemoteTimer);
}

export function pauseActiveTimer(): Promise<boolean> {
  return run(pauseRemoteTimer);
}

export function resumeActiveTimer(): Promise<boolean> {
  return run(resumeRemoteTimer);
}

/**
 * Aborts the active timer. The engine discards it and reports the empty idle
 * state, so this is not a visual reset: the display window receives the same
 * `00:00` snapshot as the control window.
 */
export function abortActiveTimer(): Promise<boolean> {
  return run(abortRemoteTimer);
}

export function stopActiveTimer(): Promise<boolean> {
  return run(stopRemoteTimer);
}
