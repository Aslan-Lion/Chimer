import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ConnectedDevice,
  DevicePermission,
  DeviceRole,
  DeviceView,
  NetworkAddress,
  NetworkError,
  NetworkStatus,
  PairingCode,
  PairingRequest,
  RemotePreset,
  RemoteSession,
} from "../types";
import { isTauriRuntime } from "./backend";

/**
 * Host-side IPC surface for Milestone 6 local networking.
 *
 * This is the control window's view of the Rust `network` module: it drives the
 * local server, the pairing flow and the paired-device list. The remote browser
 * client talks to the host over a WebSocket instead (see `services/remote.ts`);
 * the two never share a transport.
 *
 * Event channel names mirror `src-tauri/src/events` exactly. Every command is a
 * thin adapter over the authoritative `NetworkEngine`, so the frontend never
 * keeps its own device records — it re-renders whatever the host reports.
 */

/** Event channels emitted by the Rust network engine. */
export const NETWORK_EVENTS = {
  status: "network://status-changed",
  devices: "network://devices-changed",
  pairing: "network://pairing-changed",
} as const;

/**
 * A failure raised by an IPC command. `network_start` rejects with a structured
 * `NetworkError`; the remaining commands reject with a plain string. Both are
 * normalised here so callers can render a message and, when present, the stable
 * code that selects the operator guidance.
 */
export interface NetworkCommandError {
  code: string | null;
  message: string;
}

/** Normalises either Rust rejection shape into one frontend-friendly error. */
export function toNetworkError(cause: unknown): NetworkCommandError {
  if (typeof cause === "string") return { code: null, message: cause };
  if (cause && typeof cause === "object") {
    const shaped = cause as Partial<NetworkError>;
    if (typeof shaped.message === "string") {
      return { code: shaped.code ?? null, message: shaped.message };
    }
  }
  return { code: null, message: String(cause) };
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

/** Throws when called outside the desktop shell; networking has no host here. */
function requireHost(): void {
  if (!isTauriRuntime()) {
    throw new Error("Local networking is only available in the desktop app.");
  }
}

/** Reads the current server status without touching the listener. */
export async function getNetworkStatus(): Promise<NetworkStatus | null> {
  if (!isTauriRuntime()) return null;
  return invoke<NetworkStatus>("network_status");
}

/** Re-reads the machine's network interfaces and returns the fresh status. */
export async function refreshNetwork(): Promise<NetworkStatus | null> {
  if (!isTauriRuntime()) return null;
  return invoke<NetworkStatus>("network_refresh");
}

/**
 * Starts the local server. `address` may be `null` to let the host pick the
 * best local interface; `bindAll` binds every interface instead.
 */
export async function startNetwork(
  port: number,
  address: string | null,
  bindAll: boolean,
): Promise<NetworkStatus> {
  requireHost();
  return invoke<NetworkStatus>("network_start", { port, address, bindAll });
}

/** Stops the local server and drops every connection. */
export async function stopNetwork(): Promise<NetworkStatus | null> {
  if (!isTauriRuntime()) return null;
  return invoke<NetworkStatus>("network_stop");
}

/** The interfaces a device could reach this host on. */
export async function getNetworkAddresses(): Promise<NetworkAddress[]> {
  if (!isTauriRuntime()) return [];
  return invoke<NetworkAddress[]>("network_addresses");
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

/** Issues a fresh, short-lived pairing code and its QR code. */
export async function issuePairingCode(): Promise<PairingCode> {
  requireHost();
  return invoke<PairingCode>("network_issue_pairing");
}

/** Pairing requests currently waiting for the operator's decision. */
export async function getPendingRequests(): Promise<PairingRequest[]> {
  if (!isTauriRuntime()) return [];
  return invoke<PairingRequest[]>("network_pending_requests");
}

/** Approves a request and gives the device its role. */
export async function approvePairing(requestId: string, role: DeviceRole): Promise<void> {
  requireHost();
  return invoke("network_approve_pairing", { requestId, role });
}

/** Declines a request. The pairing code stays usable. */
export async function rejectPairing(requestId: string): Promise<void> {
  requireHost();
  return invoke("network_reject_pairing", { requestId });
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/** Every paired device, with its live connection state. */
export async function getDevices(): Promise<DeviceView[]> {
  if (!isTauriRuntime()) return [];
  return invoke<DeviceView[]>("network_devices");
}

/** Every live connection, including ones that have not paired yet. */
export async function getConnections(): Promise<ConnectedDevice[]> {
  if (!isTauriRuntime()) return [];
  return invoke<ConnectedDevice[]>("network_connections");
}

/** Changes a device's role. */
export async function setDeviceRole(deviceId: string, role: DeviceRole): Promise<void> {
  requireHost();
  return invoke("network_set_role", { deviceId, role });
}

/** Replaces a device's individual permissions. */
export async function setDevicePermissions(
  deviceId: string,
  permissions: DevicePermission[],
): Promise<void> {
  requireHost();
  return invoke("network_set_permissions", { deviceId, permissions });
}

/** Revokes a device: its token stops working and its sockets are closed. */
export async function revokeDevice(deviceId: string): Promise<void> {
  requireHost();
  return invoke("network_revoke_device", { deviceId });
}

/** Drops one connection immediately, without revoking the device. */
export async function disconnectDevice(connectionId: string): Promise<void> {
  requireHost();
  return invoke("network_disconnect", { connectionId });
}

// ---------------------------------------------------------------------------
// Library mirror
// ---------------------------------------------------------------------------

/**
 * Publishes the operator's presets and sessions to the host's mirror.
 *
 * The host resolves a remote's `preset.start` / `session.start` against this
 * same mirror, so a remote can only ever start something the host has. The
 * mirror is only replaced when it actually changed.
 */
export async function setNetworkLibrary(
  presets: RemotePreset[],
  sessions: RemoteSession[],
): Promise<void> {
  if (!isTauriRuntime()) return;
  return invoke("network_set_library", { presets, sessions });
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/** Fires on every authoritative server status change. */
export function onNetworkStatus(
  handler: (status: NetworkStatus) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<NetworkStatus>(NETWORK_EVENTS.status, (event) => handler(event.payload));
}

/** Fires when the paired-device list changes. */
export function onNetworkDevices(
  handler: (devices: DeviceView[]) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<DeviceView[]>(NETWORK_EVENTS.devices, (event) => handler(event.payload));
}

/** Fires when the pending pairing requests change. */
export function onNetworkPairing(
  handler: (requests: PairingRequest[]) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<PairingRequest[]>(NETWORK_EVENTS.pairing, (event) => handler(event.payload));
}
