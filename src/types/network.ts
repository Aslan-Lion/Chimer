import type {
  PresentationOptions,
  SessionPresentation,
  SessionSnapshot,
  TimerSnapshot,
} from ".";
import type { SessionDefinition } from "./session";

/**
 * Shared vocabulary for Milestone 6 local networking.
 *
 * Every interface below is a field-for-field mirror of a Rust type in
 * `src-tauri/src/network/` (`model.rs` for the host-facing shapes, `protocol.rs`
 * for the wire vocabulary). Rust serialises all of them with
 * `#[serde(rename_all = "camelCase")]`, which is why the frontend never invents
 * a shape of its own: the host is authoritative for every value here.
 */

// ---------------------------------------------------------------------------
// Host-facing vocabulary (`network/model.rs`)
// ---------------------------------------------------------------------------

/** What a paired device is allowed to do. */
export type DeviceRole = "controller" | "display";

/** Broad device family, inferred from the user agent by the host. */
export type DeviceKind = "phone" | "tablet" | "desktop" | "unknown";

/** An individual capability that can be granted or withheld per device. */
export type DevicePermission =
  | "timerStart"
  | "timerPause"
  | "timerResume"
  | "timerAbort"
  | "timerCreate"
  | "presetStart"
  | "sessionStart"
  | "sessionAdvance"
  | "sessionStop";

/** Every permission, in the order the backend exposes them. */
export const DEVICE_PERMISSIONS: DevicePermission[] = [
  "timerStart",
  "timerPause",
  "timerResume",
  "timerAbort",
  "timerCreate",
  "presetStart",
  "sessionStart",
  "sessionAdvance",
  "sessionStop",
];

/** One network interface the host could be reached on. */
export interface NetworkAddress {
  address: string;
  label: string;
  /** True for RFC 1918 / link-local ranges. */
  isPrivate: boolean;
  /** True when the interface looks like a virtual adapter. */
  isVirtual: boolean;
}

/** A failure the operator can act on; `code` is stable. Mirrors `NetworkError`. */
export interface NetworkError {
  code: string;
  message: string;
}

/** Everything the Settings page needs to describe the server. */
export interface NetworkStatus {
  enabled: boolean;
  running: boolean;
  boundAddress: string | null;
  port: number;
  /** Address a remote device should use, e.g. `http://192.168.1.20:8722`. */
  url: string | null;
  preferredAddress: string | null;
  bindAll: boolean;
  addresses: NetworkAddress[];
  connectedCount: number;
  pairedCount: number;
  error: NetworkError | null;
}

/** A paired device with its live connection state. Mirrors `DeviceView`. */
export interface DeviceView {
  id: string;
  name: string;
  kind: DeviceKind;
  role: DeviceRole;
  permissions: DevicePermission[];
  pairedAt: number;
  lastSeenAt: number;
  connected: boolean;
  connectionCount: number;
  addresses: string[];
}

/** A remote connection the host is currently holding. Mirrors `ConnectedDevice`. */
export interface ConnectedDevice {
  connectionId: string;
  deviceId: string | null;
  name: string;
  kind: DeviceKind;
  role: DeviceRole | null;
  permissions: DevicePermission[];
  address: string;
  authenticated: boolean;
  connectedAt: number;
  lastSeenAt: number;
  userAgent: string;
}

/** A device waiting for the operator's decision. Mirrors `PairingRequest`. */
export interface PairingRequest {
  requestId: string;
  name: string;
  kind: DeviceKind;
  address: string;
  userAgent: string;
  requestedAt: number;
  expiresAt: number;
}

/** A QR code plus the URL it encodes. Mirrors `PairingCode` in `qr.rs`. */
export interface PairingCode {
  code: string;
  url: string;
  svg: string;
  /** Milliseconds from now until the code stops working. */
  expiresInMs: number;
}

// ---------------------------------------------------------------------------
// Library mirror (`network/protocol.rs`)
// ---------------------------------------------------------------------------

/** A timer preset a remote controller may start. */
export interface RemotePreset {
  id: string;
  name: string;
  durationMs: number;
}

/** A session a remote controller may start. */
export interface RemoteSession {
  id: string;
  name: string;
  stageCount: number;
  definition: SessionDefinition;
}

/** The part of the operator's library a remote controller needs. */
export interface LibraryMirror {
  presets: RemotePreset[];
  sessions: RemoteSession[];
}

// ---------------------------------------------------------------------------
// Wireless protocol (`network/protocol.rs`)
// ---------------------------------------------------------------------------

/** Version of the message vocabulary; the host refuses a different one. */
export const PROTOCOL_VERSION = 1;

/** A failure a remote can be told about. Mirrors `ProtocolError`. */
export interface ProtocolError {
  code: string;
  message: string;
}

/** Host identity sent in the initial `welcome`. Mirrors `ServerInfo`. */
export interface ServerInfo {
  name: string;
  version: string;
}

/** A paired device as the device itself sees it. Mirrors `DeviceDescriptor`. */
export interface DeviceDescriptor {
  id: string;
  name: string;
  kind: DeviceKind;
  role: DeviceRole;
  permissions: DevicePermission[];
  pairedAt: number;
  lastSeenAt: number;
}

/**
 * The complete authoritative state, sent once a device is authenticated.
 * Mirrors `HostSnapshot`; every field is computed by the host.
 */
export interface HostSnapshot {
  revision: number;
  timer: TimerSnapshot | null;
  session: SessionSnapshot | null;
  sessionPresentation: SessionPresentation | null;
  presentation: PresentationOptions;
  libraryRevision: number;
  serverTimeMs: number;
}

// -- Client -> host ---------------------------------------------------------

export type ClientMessage =
  | { type: "hello"; protocol: number }
  | { type: "pair"; code: string; name?: string }
  | { type: "authenticate"; deviceId: string; token: string }
  | { type: "command"; id: string; action: string; payload?: unknown }
  | { type: "snapshot" }
  | { type: "ping"; nonce?: number };

// -- Host -> client ---------------------------------------------------------

export type ServerMessage =
  | {
      type: "welcome";
      protocol: number;
      server: ServerInfo;
      authenticated: boolean;
      role: DeviceRole | null;
      permissions: DevicePermission[];
    }
  | { type: "pairing-pending"; requestId: string; expiresAt: number }
  | { type: "paired"; device: DeviceDescriptor; token: string }
  | { type: "pairing-rejected"; requestId: string; reason: string }
  | { type: "device-updated"; role: DeviceRole; permissions: DevicePermission[] }
  | { type: "revoked"; reason: string }
  | { type: "library"; revision: number; presets: RemotePreset[]; sessions: RemoteSession[] }
  | { type: "snapshot"; snapshot: HostSnapshot }
  | {
      type: "state";
      revision: number;
      timer: TimerSnapshot | null;
      session: SessionSnapshot | null;
      sessionPresentation: SessionPresentation | null;
    }
  | { type: "presentation"; revision: number; presentation: PresentationOptions }
  | { type: "command-result"; id: string; ok: boolean; error: ProtocolError | null }
  | { type: "error"; code: string; message: string }
  | { type: "pong"; nonce: number | null };

/** The fixed set of actions a remote controller may ask the host to run. */
export const REMOTE_ACTIONS = {
  timerStart: "timer.start",
  timerPause: "timer.pause",
  timerResume: "timer.resume",
  timerAbort: "timer.abort",
  timerCreate: "timer.create",
  presetStart: "preset.start",
  sessionStart: "session.start",
  sessionAdvance: "session.advance",
  sessionStop: "session.stop",
} as const;

/** The permission each action requires, mirroring `RemoteAction::permission`. */
export const ACTION_PERMISSION: Record<string, DevicePermission> = {
  "timer.start": "timerStart",
  "timer.pause": "timerPause",
  "timer.resume": "timerResume",
  "timer.abort": "timerAbort",
  "timer.create": "timerCreate",
  "preset.start": "presetStart",
  "session.start": "sessionStart",
  "session.advance": "sessionAdvance",
  "session.stop": "sessionStop",
};

/** Human labels for the permission editor. */
export const PERMISSION_LABELS: Record<DevicePermission, string> = {
  timerStart: "Start a timer",
  timerPause: "Pause a timer",
  timerResume: "Resume a timer",
  timerAbort: "Abort a timer",
  timerCreate: "Create or configure a timer",
  presetStart: "Start a timer preset",
  sessionStart: "Start a session",
  sessionAdvance: "Advance a session",
  sessionStop: "Stop a session",
};
