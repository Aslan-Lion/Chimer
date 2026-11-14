import type {
  ConnectedDevice,
  DevicePermission,
  DeviceRole,
  DeviceView,
  NetworkStatus,
  PairingCode,
  PairingRequest,
  RemotePreset,
  RemoteSession,
} from "../types";
import { createStore } from "./createStore";
import { libraryStore } from "./libraryStore";
import {
  approvePairing,
  disconnectDevice as disconnectRemoteDevice,
  getConnections,
  getDevices,
  getNetworkStatus,
  getPendingRequests,
  issuePairingCode,
  onNetworkDevices,
  onNetworkPairing,
  onNetworkStatus,
  refreshNetwork as refreshRemoteNetwork,
  rejectPairing,
  revokeDevice as revokeRemoteDevice,
  setDevicePermissions,
  setDeviceRole,
  setNetworkLibrary,
  startNetwork,
  stopNetwork,
  toNetworkError,
  type NetworkCommandError,
} from "../services/network";
import { isControlWindow } from "../services/backend";

/**
 * Control-window mirror of the Rust network engine.
 *
 * The host is authoritative: the store holds no device record of its own, it
 * only reflects the `NetworkStatus`, the `DeviceView` list and the pending
 * `PairingRequest`s the engine broadcasts. Every mutating action is a command
 * whose result arrives both as a return value and as a fresh broadcast, so the
 * two windows can never disagree about who is paired or connected.
 */
interface NetworkStoreState {
  /** Last status broadcast by the engine, or `null` before the first read. */
  status: NetworkStatus | null;
  /** Every paired device, with its live connection state. */
  devices: DeviceView[];
  /** Every live connection, including ones that have not paired yet. */
  connections: ConnectedDevice[];
  /** Pairing requests waiting for the operator's decision. */
  pending: PairingRequest[];
  /** Last issued pairing code, or `null` once it has been consumed. */
  pairing: PairingCode | null;
  /** Local timestamp at which `pairing` was issued, for the expiry countdown. */
  pairingIssuedAt: number | null;
  busy: boolean;
  error: NetworkCommandError | null;
}

export const networkStore = createStore<NetworkStoreState>({
  status: null,
  devices: [],
  connections: [],
  pending: [],
  pairing: null,
  pairingIssuedAt: null,
  busy: false,
  error: null,
});

/** Applies an authoritative status broadcast. */
export function applyNetworkStatus(status: NetworkStatus): void {
  networkStore.set({ status });
}

/** Applies an authoritative device list broadcast. */
export function applyNetworkDevices(devices: DeviceView[]): void {
  networkStore.set({ devices });
}

/** Applies an authoritative pending-request broadcast. */
export function applyNetworkPairing(pending: PairingRequest[]): void {
  networkStore.set({ pending });
}

/** Takes ownership of the pending error so a caller can report it once. */
export function clearNetworkError(): NetworkCommandError | null {
  const { error } = networkStore.get();
  if (error !== null) networkStore.set({ error: null });
  return error;
}

/**
 * Runs one host command, holding the UI while it is in flight.
 *
 * On success the local `connections` list is refreshed, because the engine
 * broadcasts status and devices but a connect/disconnect does not change the
 * paired set. On failure the structured error is kept for the caller to render.
 */
async function run(
  command: () => Promise<unknown>,
  options: { refreshConnections?: boolean } = {},
): Promise<boolean> {
  networkStore.set({ busy: true, error: null });
  try {
    await command();
    if (options.refreshConnections) {
      const connections = await getConnections();
      networkStore.set({ connections });
    }
    networkStore.set({ busy: false });
    return true;
  } catch (cause) {
    networkStore.set({ busy: false, error: toNetworkError(cause) });
    return false;
  }
}

/** Reads the full host networking state; safe while networking is disabled. */
export async function loadNetwork(): Promise<void> {
  try {
    const [status, devices, pending, connections] = await Promise.all([
      getNetworkStatus(),
      getDevices(),
      getPendingRequests(),
      getConnections(),
    ]);
    networkStore.set({
      ...(status ? { status } : {}),
      devices,
      pending,
      connections,
    });
  } catch (cause) {
    networkStore.set({ error: toNetworkError(cause) });
  }
}

/** Re-reads the machine's interfaces on the host. */
export async function refreshNetwork(): Promise<boolean> {
  networkStore.set({ busy: true, error: null });
  try {
    const status = await refreshRemoteNetwork();
    if (status) networkStore.set({ status });
    networkStore.set({ busy: false });
    return true;
  } catch (cause) {
    networkStore.set({ busy: false, error: toNetworkError(cause) });
    return false;
  }
}

/** Turns the local server on. */
export function startServer(
  port: number,
  address: string | null,
  bindAll: boolean,
): Promise<boolean> {
  return run(() => startNetwork(port, address, bindAll));
}

/** Turns the local server off, dropping every connection. */
export function stopServer(): Promise<boolean> {
  networkStore.set({ pairing: null, pairingIssuedAt: null });
  return run(() => stopNetwork());
}

/** Issues a fresh pairing code and remembers when it was created. */
export async function issuePairing(): Promise<PairingCode | null> {
  networkStore.set({ busy: true, error: null });
  try {
    const code = await issuePairingCode();
    networkStore.set({ pairing: code, pairingIssuedAt: Date.now(), busy: false });
    return code;
  } catch (cause) {
    networkStore.set({ busy: false, error: toNetworkError(cause) });
    return null;
  }
}

/** Drops the displayed pairing code (the host has consumed or expired it). */
export function forgetPairing(): void {
  networkStore.set({ pairing: null, pairingIssuedAt: null });
}

/** Approves a pending request and assigns the device its role. */
export function approveRequest(requestId: string, role: DeviceRole): Promise<boolean> {
  return run(() => approvePairing(requestId, role));
}

/** Declines a pending request. */
export function rejectRequest(requestId: string): Promise<boolean> {
  return run(() => rejectPairing(requestId));
}

/** Changes a device's role. */
export function changeDeviceRole(deviceId: string, role: DeviceRole): Promise<boolean> {
  return run(() => setDeviceRole(deviceId, role));
}

/** Replaces a device's permissions. */
export function changeDevicePermissions(
  deviceId: string,
  permissions: DevicePermission[],
): Promise<boolean> {
  return run(() => setDevicePermissions(deviceId, permissions));
}

/** Revokes a device: its token stops working and its sockets are closed. */
export function revoke(deviceId: string): Promise<boolean> {
  return run(() => revokeRemoteDevice(deviceId));
}

/** Disconnects one connection without revoking the device. */
export function disconnect(connectionId: string): Promise<boolean> {
  return run(() => disconnectRemoteDevice(connectionId), { refreshConnections: true });
}

/**
 * Publishes the operator's library to the host mirror.
 *
 * Only the ids, names and durations travel for presets; a session sends its
 * whole definition so the host can run it. A remote can therefore only ever
 * start something the host actually holds — a fabricated id is refused by the
 * engine, not by the browser.
 */
export async function publishLibrary(): Promise<void> {
  const { presets, sessions } = libraryStore.get();
  const remotePresets: RemotePreset[] = presets.map((preset) => ({
    id: preset.id,
    name: preset.name,
    durationMs: preset.durationMs,
  }));
  const remoteSessions: RemoteSession[] = sessions.map((session) => ({
    id: session.id,
    name: session.name,
    stageCount: session.stages.length,
    definition: session,
  }));
  try {
    await setNetworkLibrary(remotePresets, remoteSessions);
  } catch (cause) {
    networkStore.set({ error: toNetworkError(cause) });
  }
}

/**
 * Subscribes to the host broadcasts, loads the current state and keeps the
 * library mirror in step with the local library.
 *
 * The listeners are registered before the initial read, so an update that
 * arrives in between cannot be missed. The returned function detaches every
 * listener; the library subscription is a no-op outside the control window,
 * because only the operator's machine owns the library.
 */
export async function startNetworkBridge(): Promise<() => void> {
  const [offStatus, offDevices, offPairing] = await Promise.all([
    onNetworkStatus(applyNetworkStatus),
    onNetworkDevices(applyNetworkDevices),
    onNetworkPairing(applyNetworkPairing),
  ]);

  await loadNetwork();

  const ownsLibrary = isControlWindow();
  if (ownsLibrary) {
    await publishLibrary();
  }
  const offLibrary = ownsLibrary
    ? libraryStore.subscribe(() => {
        void publishLibrary();
      })
    : () => undefined;

  return () => {
    offStatus();
    offDevices();
    offPairing();
    offLibrary();
  };
}
