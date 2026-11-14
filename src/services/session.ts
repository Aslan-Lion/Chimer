import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  SessionCue,
  SessionDefinition,
  SessionPresentation,
  SessionSnapshot,
} from "../types";
import { isTauriRuntime } from "./backend";

/**
 * Event channels emitted by the Rust session engine. The names mirror
 * `src-tauri/src/events`. Both windows subscribe to the same channels, so the
 * control window and the presentation window can never disagree about the
 * current stage.
 */
export const SESSION_STATE_CHANGED = "session://state-changed";
export const SESSION_CUE = "session://cue";
export const DISPLAY_SESSION_CHANGED = "display://session-changed";

/**
 * Every session command goes through the authoritative engine, which returns
 * the resulting cursor *and* broadcasts it. Outside the desktop shell there is
 * no engine, and inventing a stage progression in the browser would be a lie —
 * the commands fail loudly instead.
 */
function requireEngine(): void {
  if (!isTauriRuntime()) {
    throw new Error("The session engine is only available in the desktop app.");
  }
}

/** Reads the current session cursor. Used on startup so a window never renders stale state. */
export async function getSessionSnapshot(): Promise<SessionSnapshot | null> {
  if (!isTauriRuntime()) return null;
  return invoke<SessionSnapshot>("session_snapshot");
}

/** Loads a session and starts its first stage. */
export async function startSession(definition: SessionDefinition): Promise<SessionSnapshot> {
  requireEngine();
  return invoke<SessionSnapshot>("session_start", { definition });
}

/** Advances to the next stage (the operator's action at a manual transition). */
export async function nextStage(): Promise<SessionSnapshot> {
  requireEngine();
  return invoke<SessionSnapshot>("session_next");
}

/** Ends the session, returning both windows to the idle presentation. */
export async function stopSession(): Promise<SessionSnapshot> {
  requireEngine();
  return invoke<SessionSnapshot>("session_stop");
}

/** Fires on every authoritative session cursor update from Rust. */
export function onSessionState(
  handler: (snapshot: SessionSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<SessionSnapshot>(SESSION_STATE_CHANGED, (event) => handler(event.payload));
}

/** Fires for each discrete session moment (stage start, warning, completion). */
export function onSessionCue(handler: (cue: SessionCue) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<SessionCue>(SESSION_CUE, (event) => handler(event.payload));
}

/**
 * Reads the stage the session engine last pushed to the display window, or
 * `null` when no session is running. The display window calls this on startup.
 */
export async function getSessionPresentation(): Promise<SessionPresentation | null> {
  if (!isTauriRuntime()) return null;
  return invoke<SessionPresentation | null>("session_presentation");
}

/** Fires when the session engine changes the stage shown on the display window. */
export function onSessionPresentationChanged(
  handler: (presentation: SessionPresentation | null) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<SessionPresentation | null>(DISPLAY_SESSION_CHANGED, (event) =>
    handler(event.payload),
  );
}
