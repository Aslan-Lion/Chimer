import type {
  DisplayGeometry,
  DisplayWindowState,
  MonitorInfo,
  PresentationOptions,
} from "../types";
import { CLOSED_DISPLAY_WINDOW } from "../types";
import { readStorage, writeStorage } from "../lib/storage";
import * as api from "../services/display";
import { createStore } from "./createStore";
import { settingsStore } from "./settingsStore";

/**
 * Placement of the presentation window survives an application restart, so the
 * control window remembers it locally and Rust re-validates it against the
 * connected displays every time the window opens.
 */
const GEOMETRY_KEY = "app-timer.display-geometry";

export type MonitorLoadState = "loading" | "ready" | "error";

interface DisplayStoreState {
  /** Live lifecycle state of the native presentation window. */
  windowState: DisplayWindowState;
  monitors: MonitorInfo[];
  monitorsState: MonitorLoadState;
  monitorsError: string | null;
  /** Last observed placement of the presentation window. */
  geometry: DisplayGeometry | null;
  /** Monitor the presentation window should open on or move to. */
  targetMonitorKey: string | null;
  /** True while a native window command is in flight. */
  busy: boolean;
  error: string | null;
}

export const displayStore = createStore<DisplayStoreState>({
  windowState: CLOSED_DISPLAY_WINDOW,
  monitors: [],
  monitorsState: "loading",
  monitorsError: null,
  geometry: readStorage<DisplayGeometry | null>(GEOMETRY_KEY, null),
  targetMonitorKey: null,
  busy: false,
  error: null,
});

function describe(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function rememberGeometry(geometry: DisplayGeometry): void {
  displayStore.set({ geometry });
  writeStorage(GEOMETRY_KEY, geometry);
}

// ---------------------------------------------------------------------------
// Incoming event payloads
// ---------------------------------------------------------------------------

/** Applies a `display://state-changed` payload. */
export function applyWindowState(windowState: DisplayWindowState): void {
  displayStore.set({ windowState });
}

/** Applies a `display://monitors-changed` payload, re-homing a stale selection. */
export function applyMonitors(monitors: MonitorInfo[]): void {
  displayStore.set((previous) => {
    const keys = new Set(monitors.map((monitor) => monitor.key));
    let target = previous.targetMonitorKey;

    if (target !== null && !keys.has(target)) target = previous.windowState.monitorKey;
    if (target === null || !keys.has(target)) {
      target = monitors.find((monitor) => monitor.isPrimary)?.key ?? monitors[0]?.key ?? null;
    }

    return {
      monitors,
      monitorsState: "ready" as MonitorLoadState,
      monitorsError: null,
      targetMonitorKey: target,
    };
  });
}

/** Applies a `display://geometry` payload. */
export function applyGeometry(geometry: DisplayGeometry): void {
  rememberGeometry(geometry);
}

// ---------------------------------------------------------------------------
// Window commands
// ---------------------------------------------------------------------------

async function run(action: () => Promise<DisplayWindowState>): Promise<void> {
  if (!api.isDesktopRuntime()) {
    displayStore.set({ error: "The display window is only available in the desktop app." });
    return;
  }

  displayStore.set({ busy: true, error: null });
  try {
    const windowState = await action();
    displayStore.set({ windowState, busy: false });
  } catch (cause) {
    displayStore.set({ busy: false, error: describe(cause) });
  }
}

/** Re-reads the connected displays and the live window state. */
export async function refreshDisplay(): Promise<void> {
  if (!api.isDesktopRuntime()) {
    displayStore.set({ monitorsState: "ready", monitors: [] });
    return;
  }

  try {
    const monitors = await api.syncMonitors();
    applyMonitors(monitors);
  } catch (cause) {
    displayStore.set({ monitorsState: "error", monitorsError: describe(cause) });
  }

  try {
    applyWindowState(await api.getDisplayWindowState());
  } catch (cause) {
    displayStore.set({ error: describe(cause) });
  }
}

/** Opens the presentation window on the selected monitor at its last placement. */
export async function openDisplay(): Promise<void> {
  const { settings } = settingsStore.get();
  const { geometry, targetMonitorKey } = displayStore.get();

  await run(() =>
    api.openDisplayWindow({
      monitorKey: targetMonitorKey ?? settings.display.defaultMonitorKey,
      x: geometry?.x ?? null,
      y: geometry?.y ?? null,
      width: geometry?.width ?? settings.display.defaultWidth,
      height: geometry?.height ?? settings.display.defaultHeight,
      fullscreen: settings.display.startFullscreen,
      alwaysOnTop: settings.display.alwaysOnTop,
    }),
  );
}

/** Closes the presentation window; the control window stays operational. */
export async function closeDisplay(): Promise<void> {
  await run(() => api.closeDisplayWindow());
}

/** Toggles the presentation window between windowed and fullscreen. */
export async function toggleDisplayFullscreen(): Promise<void> {
  const { windowState } = displayStore.get();
  if (!windowState.open) return;
  await run(() => api.setDisplayFullscreen(!windowState.fullscreen));
}

/** Returns the presentation window to a visible, usable windowed state. */
export async function restoreDisplay(): Promise<void> {
  await run(() => api.restoreDisplayWindow());
}

/** Pins or unpins the presentation window above other windows. */
export async function toggleDisplayAlwaysOnTop(): Promise<void> {
  const { windowState } = displayStore.get();
  if (!windowState.open) return;
  await run(() => api.setDisplayAlwaysOnTop(!windowState.alwaysOnTop));
}

/** Switches the presentation window between maximised and windowed. */
export async function toggleDisplayMaximized(): Promise<void> {
  const { windowState } = displayStore.get();
  if (!windowState.open) return;
  await run(() => api.setDisplayMaximized(!windowState.maximized));
}

/**
 * Enters or leaves presentation mode.
 *
 * Rust captures the window mode on entry and restores it on exit, so leaving
 * presentation mode returns the window exactly where it was rather than to a
 * default size. The presentation window is moved and resized, never replaced.
 */
export async function toggleDisplayPresentation(): Promise<void> {
  const { windowState } = displayStore.get();
  if (!windowState.open) return;
  await run(() => api.setDisplayPresentation(!windowState.presentation));
}

/** Moves the presentation window onto the given monitor. */
export async function moveDisplayToMonitor(monitorKey: string): Promise<void> {
  await run(() => api.moveDisplayToMonitor(monitorKey));
}

/**
 * Records the monitor the user picked and, when the window is already open,
 * moves it there immediately so the choice is always observable.
 */
export async function selectDisplayMonitor(monitorKey: string): Promise<void> {
  displayStore.set({ targetMonitorKey: monitorKey, error: null });
  if (displayStore.get().windowState.open) await moveDisplayToMonitor(monitorKey);
}

/** First monitor that is neither the primary nor the one hosting the control window. */
export function secondMonitor(): MonitorInfo | null {
  const { monitors } = displayStore.get();
  return (
    monitors.find((monitor) => !monitor.isPrimary && !monitor.isCurrent) ??
    monitors.find((monitor) => !monitor.isPrimary) ??
    null
  );
}

/** Moves the presentation window onto the secondary display. */
export async function moveToSecondMonitor(): Promise<void> {
  const target = secondMonitor();
  if (!target) {
    displayStore.set({ error: "Only one display is connected." });
    return;
  }
  await selectDisplayMonitor(target.key);
}

/** Pushes the shared presentation options to Rust, which broadcasts them. */
export async function syncPresentationOptions(options: PresentationOptions): Promise<void> {
  if (!api.isDesktopRuntime()) return;
  try {
    await api.setPresentationOptions(options);
  } catch (cause) {
    displayStore.set({ error: describe(cause) });
  }
}

/** Clears the last surfaced error. */
export function clearDisplayError(): void {
  displayStore.set({ error: null });
}
