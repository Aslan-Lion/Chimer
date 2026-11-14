import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TimerSnapshot } from "../types";
import { isTauriRuntime } from "./backend";

/**
 * Full snapshot broadcast by the Rust engine whenever the timer changes and,
 * while it runs or accrues overtime, on every heartbeat. The name mirrors
 * `src-tauri/src/events`. Rust also emits `timer://completed` once per
 * completion; everything needed to render is already in the state channel, so
 * this build only subscribes to this one.
 */
export const TIMER_STATE_CHANGED = "timer://state-changed";

/**
 * Every timer command goes through the same authoritative engine, which returns
 * the resulting snapshot *and* broadcasts it, so the calling window and the
 * display window always converge on the same value.
 *
 * Outside the desktop shell there is no engine to talk to, and inventing a
 * countdown in the browser would be a lie — the commands therefore fail loudly
 * instead.
 */
function requireEngine(): void {
  if (!isTauriRuntime()) {
    throw new Error("The timer engine is only available in the desktop app.");
  }
}

/** Reads the current snapshot. Used on startup so a window never renders stale state. */
export async function getTimerSnapshot(): Promise<TimerSnapshot | null> {
  if (!isTauriRuntime()) return null;
  return invoke<TimerSnapshot>("timer_snapshot");
}

/** Configures a new timer, replacing any previous one. */
export async function createTimer(name: string, durationMs: number): Promise<TimerSnapshot> {
  requireEngine();
  return invoke<TimerSnapshot>("timer_create", { name, durationMs });
}

/** Starts the countdown from the full configured duration. */
export async function startTimer(): Promise<TimerSnapshot> {
  requireEngine();
  return invoke<TimerSnapshot>("timer_start");
}

/** Freezes the countdown, preserving the exact remaining time. */
export async function pauseTimer(): Promise<TimerSnapshot> {
  requireEngine();
  return invoke<TimerSnapshot>("timer_pause");
}

/** Continues a paused countdown from where it stopped. */
export async function resumeTimer(): Promise<TimerSnapshot> {
  requireEngine();
  return invoke<TimerSnapshot>("timer_resume");
}

/**
 * Aborts the timer outright, returning the engine to the empty idle state:
 * `00:00` displayed, no progress and no overtime counter.
 */
export async function abortTimer(): Promise<TimerSnapshot> {
  requireEngine();
  return invoke<TimerSnapshot>("timer_abort");
}

/** Terminates the countdown, preserving the remaining time. */
export async function stopTimer(): Promise<TimerSnapshot> {
  requireEngine();
  return invoke<TimerSnapshot>("timer_stop");
}

/** Adjusts the active timer through the authoritative Rust state machine. */
export async function adjustTimer(deltaMs: number): Promise<TimerSnapshot> {
  requireEngine();
  if (!Number.isSafeInteger(deltaMs) || deltaMs === 0) {
    throw new Error("Timer adjustment must be a non-zero whole number of milliseconds.");
  }
  return invoke<TimerSnapshot>("timer_adjust", { deltaMs });
}

/** Fires on every authoritative timer update from Rust. */
export function onTimerState(
  handler: (snapshot: TimerSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<TimerSnapshot>(TIMER_STATE_CHANGED, (event) => handler(event.payload));
}
