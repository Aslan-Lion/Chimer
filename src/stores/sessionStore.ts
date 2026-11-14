import type { SessionCue, SessionDefinition, SessionSnapshot } from "../types";
import { createStore } from "./createStore";
import { playSound } from "../lib/sound";
import { isControlWindow } from "../services/backend";
import { libraryStore } from "./libraryStore";
import { settingsStore } from "./settingsStore";
import {
  getSessionSnapshot,
  nextStage,
  onSessionCue,
  onSessionState,
  startSession as startRemoteSession,
  stopSession as stopRemoteSession,
} from "../services/session";

interface SessionStoreState {
  /**
   * Last cursor broadcast by the Rust engine, or `null` before the first one
   * arrives. A snapshot whose `sessionId` is `null` means no session is loaded.
   * The store holds no clock of its own: the stage countdown is the timer
   * snapshot's, so it is a pure mirror, never a counter.
   */
  snapshot: SessionSnapshot | null;
  /** Last discrete cue, kept for diagnostics and lightweight UI hints. */
  lastCue: SessionCue | null;
  busy: boolean;
  error: string | null;
}

export const sessionStore = createStore<SessionStoreState>({
  snapshot: null,
  lastCue: null,
  busy: false,
  error: null,
});

function describe(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** Applies an authoritative cursor. Both windows receive it through this path. */
export function applySessionSnapshot(snapshot: SessionSnapshot): void {
  sessionStore.set({ snapshot });
}

/** Takes ownership of the pending error message so it is reported once. */
export function clearSessionError(): string | null {
  const { error } = sessionStore.get();
  if (error !== null) sessionStore.set({ error: null });
  return error;
}

/**
 * Plays the sound a cue calls for.
 *
 * The cue only says *whether* a sound was requested and at which moment; the
 * sound itself is the running stage's message sound, falling back to the
 * configured completion sound for the end of a session and to a plain chime for
 * everything else. Each cue kind has its own switch, so an operator can keep the
 * stage chimes and silence the warnings.
 *
 * Only the control window plays anything: both windows run the same bundle on
 * the same machine, and playing on both would double every notification. Cues
 * are discrete engine events and are never re-sent, so nothing replays after a
 * restart either.
 */
function handleCue(cue: SessionCue): void {
  sessionStore.set({ lastCue: cue });
  if (!isControlWindow()) return;

  const { settings } = settingsStore.get();
  const notifications = settings.notifications;
  if (!notifications.soundsEnabled) return;
  if (!cue.sound) return;

  if (cue.kind === "warning" && !notifications.warningSounds) return;
  if (cue.kind === "stage-start" && !notifications.stageSounds) return;
  if (cue.kind === "session-complete" && !notifications.sessionSounds) return;

  const { sessions, messages } = libraryStore.get();
  const stage = sessions
    .find((session) => session.id === cue.sessionId)
    ?.stages[cue.stageIndex];
  // `StageMessage` is a snapshot with no sound of its own: the sound belongs to
  // the message library entry, so it is resolved there by id.
  const messageId = stage?.message?.id;
  const messageSound =
    (messageId ? messages.find((message) => message.id === messageId)?.sound : null) ?? "none";

  if (messageSound !== "none") {
    playSound(messageSound);
    return;
  }

  playSound(cue.kind === "session-complete" ? notifications.completionSound : "chime");
}

/** Reads the current cursor; used on startup and after the bridge is attached. */
export async function loadSessionSnapshot(): Promise<void> {
  try {
    const snapshot = await getSessionSnapshot();
    if (snapshot) applySessionSnapshot(snapshot);
  } catch (cause) {
    sessionStore.set({ error: describe(cause) });
  }
}

/**
 * Subscribes to the Rust broadcasts and loads the current cursor.
 *
 * The listener is registered *before* the initial read, so an update that
 * happens in between cannot be missed. The returned function detaches it.
 */
export async function startSessionBridge(): Promise<() => void> {
  const [offState, offCue] = await Promise.all([
    onSessionState(applySessionSnapshot),
    onSessionCue(handleCue),
  ]);
  await loadSessionSnapshot();
  return () => {
    offState();
    offCue();
  };
}

/**
 * Runs one engine command. The engine broadcasts the resulting cursor to both
 * windows itself; the value it returns is applied here as well so the calling
 * window reacts without waiting for the round trip.
 */
async function run(command: () => Promise<SessionSnapshot>): Promise<boolean> {
  sessionStore.set({ busy: true, error: null });
  try {
    applySessionSnapshot(await command());
    sessionStore.set({ busy: false });
    return true;
  } catch (cause) {
    sessionStore.set({ busy: false, error: describe(cause) });
    return false;
  }
}

/**
 * Loads a definition into the engine and starts its first stage.
 *
 * The definition is validated by Rust, which also owns the resulting cursor;
 * every stage is run by the timer engine, so no frontend countdown exists.
 */
export function startSession(definition: SessionDefinition): Promise<boolean> {
  sessionStore.set({ busy: true, error: null });
  return startRemoteSession(definition)
    .then((snapshot) => {
      applySessionSnapshot(snapshot);
      sessionStore.set({ busy: false });
      return true;
    })
    .catch((cause) => {
      sessionStore.set({ busy: false, error: describe(cause) });
      return false;
    });
}

/** Advances to the next stage (the operator's action at a manual transition). */
export function advanceSessionStage(): Promise<boolean> {
  return run(nextStage);
}

/** Ends the session; both windows return to the idle presentation. */
export function stopSessionRun(): Promise<boolean> {
  return run(stopRemoteSession);
}
