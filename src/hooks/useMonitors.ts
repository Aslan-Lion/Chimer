import { useCallback, useEffect } from "react";
import type { MonitorInfo } from "../types";
import { refreshDisplay, displayStore } from "../stores/displayStore";
import { isDesktopRuntime } from "../services/display";

export type MonitorLoadState = "loading" | "ready" | "error";

interface MonitorsResult {
  monitors: MonitorInfo[];
  state: MonitorLoadState;
  error: string | null;
  refresh: () => Promise<void>;
  isDesktopRuntime: boolean;
}

/**
 * Detects the monitors connected to the machine.
 *
 * Windows are only movable between real displays, so the hook degrades cleanly
 * to an empty list when the UI runs in a browser during development. The data
 * itself lives in `displayStore`, so every consumer observes the same set the
 * control window uses to position the presentation window.
 */
export function useMonitors(): MonitorsResult {
  const { monitors, monitorsState, monitorsError } = displayStore.use();

  const refresh = useCallback(async () => {
    await refreshDisplay();
  }, []);

  useEffect(() => {
    if (displayStore.get().monitorsState === "loading") void refreshDisplay();
  }, []);

  return {
    monitors,
    state: monitorsState,
    error: monitorsError,
    refresh,
    isDesktopRuntime: isDesktopRuntime(),
  };
}
