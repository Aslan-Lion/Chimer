import type {
  DeviceDescriptor,
  DevicePermission,
  DeviceRole,
  PresentationOptions,
  ProtocolError,
  RemotePreset,
  RemoteSession,
  ServerInfo,
  SessionPresentation,
  SessionSnapshot,
  TimerSnapshot,
} from "../types";
import { DEFAULT_PRESENTATION_OPTIONS } from "../lib/display";
import { createStore } from "./createStore";

/**
 * State of a *remote* browser that reached the host over the LAN.
 *
 * This store lives only inside a remote page; it is never mounted in the two
 * native windows. Like every other store in the project it holds no clock and
 * computes nothing: the timer, the stage, the progress and the library are the
 * host's values, copied straight out of the protocol messages. The remote
 * controller therefore cannot drift from the desktop, and the remote display
 * cannot invent a countdown, because neither has one to invent.
 */

/** Where the remote connection currently stands. */
export type RemotePhase =
  | "idle"
  /** Opening the socket, or waiting for the first `welcome`. */
  | "connecting"
  /** Connected but unpaired: a pairing code is required. */
  | "pairing"
  /** A code was accepted; the host operator has not decided yet. */
  | "awaiting-approval"
  /** The operator approved: the token is stored, the first snapshot is due. */
  | "authenticating"
  /** Authenticated: state and library are flowing. */
  | "online"
  /** The attempt failed outright — the host never answered, it refused, or the
   * operator declined. The interface offers to try again. */
  | "failed"
  /** The code, or the request built from it, ran out of time. A new code is
   * needed; the interface says so instead of waiting forever. */
  | "expired"
  /** The host revoked this device; the stored token is useless. */
  | "revoked";

export interface RemoteState {
  phase: RemotePhase;
  /** True while the WebSocket is open, independent of authentication. */
  connected: boolean;
  /** Number of consecutive failed connection attempts, for the backoff copy. */
  reconnectAttempt: number;
  server: ServerInfo | null;
  /** The paired device as the host describes it; `null` until paired. */
  device: DeviceDescriptor | null;
  role: DeviceRole | null;
  permissions: DevicePermission[];
  timer: TimerSnapshot | null;
  session: SessionSnapshot | null;
  sessionPresentation: SessionPresentation | null;
  presentation: PresentationOptions;
  presets: RemotePreset[];
  sessions: RemoteSession[];
  /** Highest state revision applied; used to drop stale updates. */
  revision: number;
  /** Highest library revision applied. */
  libraryRevision: number;
  /** A protocol error the host reported outright (not a command rejection). */
  notice: ProtocolError | null;
  /** The last command rejection, so the UI can explain a refused action. */
  commandError: ProtocolError | null;
  /** Action string of the command currently in flight, if any. */
  pendingAction: string | null;
  /** True when a device token is held in this browser. */
  hasCredentials: boolean;
}

const INITIAL: RemoteState = {
  phase: "idle",
  connected: false,
  reconnectAttempt: 0,
  server: null,
  device: null,
  role: null,
  permissions: [],
  timer: null,
  session: null,
  sessionPresentation: null,
  presentation: DEFAULT_PRESENTATION_OPTIONS,
  presets: [],
  sessions: [],
  revision: 0,
  libraryRevision: 0,
  notice: null,
  commandError: null,
  pendingAction: null,
  hasCredentials: false,
};

export const remoteStore = createStore<RemoteState>(INITIAL);

/** Merges an authoritative patch into the mirror. */
export function applyRemote(patch: Partial<RemoteState>): void {
  remoteStore.set(patch);
}

/** Returns to the pre-connection state without clearing stored credentials. */
export function resetRemote(): void {
  remoteStore.set({ ...INITIAL, hasCredentials: remoteStore.get().hasCredentials });
}

/** Displays a one-off protocol error and clears it once the caller reads it. */
export function clearCommandError(): void {
  if (remoteStore.get().commandError !== null) remoteStore.set({ commandError: null });
}
