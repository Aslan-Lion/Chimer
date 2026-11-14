import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DisplayGeometry,
  DisplayWindowState,
  MonitorInfo,
  OpenDisplayOptions,
  PresentationOptions,
} from "../types";
import { CLOSED_DISPLAY_WINDOW } from "../types";
import { DEFAULT_PRESENTATION_OPTIONS } from "../lib/display";
import { isTauriRuntime } from "./backend";

/** Label of the native presentation window created in Milestone 2. */
export const DISPLAY_WINDOW_LABEL = "display";

/**
 * Event channels emitted by Rust. The names mirror `src-tauri/src/events`.
 * Both windows subscribe to the same channels, so either side can observe the
 * other without holding a direct reference to it.
 */
export const DISPLAY_EVENTS = {
  windowState: "display://state-changed",
  monitors: "display://monitors-changed",
  geometry: "display://geometry",
  presentation: "display://presentation-changed",
} as const;

/** True when the app runs inside the Tauri shell rather than a browser tab. */
export function isDesktopRuntime(): boolean {
  return isTauriRuntime();
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/** Enumerates every display Windows currently reports. */
export async function listMonitors(): Promise<MonitorInfo[]> {
  if (!isTauriRuntime()) return [];
  return invoke<MonitorInfo[]>("list_monitors");
}

/** Re-reads the connected displays, emitting `monitors-changed` when they differ. */
export async function syncMonitors(): Promise<MonitorInfo[]> {
  if (!isTauriRuntime()) return [];
  return invoke<MonitorInfo[]>("sync_display");
}

/** Reads the live state of the presentation window from the OS. */
export async function getDisplayWindowState(): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("display_window_state");
}

/** Opens the presentation window, or brings the existing one forward. */
export async function openDisplayWindow(
  options: OpenDisplayOptions = {},
): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("open_display_window", { options });
}

/** Closes the presentation window, leaving the control window untouched. */
export async function closeDisplayWindow(): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("close_display_window");
}

/** Switches the presentation window between windowed and fullscreen modes. */
export async function setDisplayFullscreen(enabled: boolean): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("set_display_fullscreen", { enabled });
}

/** Restores the presentation window from fullscreen / maximised / minimised. */
export async function restoreDisplayWindow(): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("restore_display_window");
}

/** Pins or unpins the presentation window above other windows. */
export async function setDisplayAlwaysOnTop(enabled: boolean): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("set_display_always_on_top", { enabled });
}

/** Maximises or restores the presentation window. */
export async function setDisplayMaximized(enabled: boolean): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("set_display_maximized", { enabled });
}

/**
 * Enters or leaves presentation mode: fullscreen, always-on-top and a
 * chrome-free frame at once, restoring the previous window mode on exit.
 */
export async function setDisplayPresentation(enabled: boolean): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("set_display_presentation", { enabled });
}

/** Moves (or opens) the presentation window on the requested monitor. */
export async function moveDisplayToMonitor(monitorKey: string): Promise<DisplayWindowState> {
  if (!isTauriRuntime()) return CLOSED_DISPLAY_WINDOW;
  return invoke<DisplayWindowState>("move_display_to_monitor", { monitorKey });
}

/** Stores and broadcasts the shared presentation options. */
export async function setPresentationOptions(
  options: PresentationOptions,
): Promise<PresentationOptions> {
  if (!isTauriRuntime()) return options;
  return invoke<PresentationOptions>("set_presentation_options", { options });
}

/** Reads the last presentation options pushed by the control window. */
export async function getPresentationOptions(): Promise<PresentationOptions> {
  if (!isTauriRuntime()) return DEFAULT_PRESENTATION_OPTIONS;
  return invoke<PresentationOptions>("presentation_options");
}

// ---------------------------------------------------------------------------
// Event subscriptions
// ---------------------------------------------------------------------------

function subscribe<T>(channel: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => undefined);
  return listen<T>(channel, (event) => handler(event.payload));
}

/** Fires whenever the presentation window opens, closes, moves or changes mode. */
export function onDisplayWindowState(
  handler: (state: DisplayWindowState) => void,
): Promise<UnlistenFn> {
  return subscribe(DISPLAY_EVENTS.windowState, handler);
}

/** Fires when a monitor is attached or removed. */
export function onMonitorsChanged(
  handler: (monitors: MonitorInfo[]) => void,
): Promise<UnlistenFn> {
  return subscribe(DISPLAY_EVENTS.monitors, handler);
}

/** Fires with the physical placement of the presentation window. */
export function onDisplayGeometry(
  handler: (geometry: DisplayGeometry) => void,
): Promise<UnlistenFn> {
  return subscribe(DISPLAY_EVENTS.geometry, handler);
}

/** Fires when the control window pushes new presentation options. */
export function onPresentationChanged(
  handler: (options: PresentationOptions) => void,
): Promise<UnlistenFn> {
  return subscribe(DISPLAY_EVENTS.presentation, handler);
}
