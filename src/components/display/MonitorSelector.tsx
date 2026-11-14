import { MonitorList } from "./MonitorList";
import { isDesktopRuntime } from "../../services/display";
import { displayStore, refreshDisplay, selectDisplayMonitor } from "../../stores/displayStore";

/**
 * Monitor picker for the presentation window.
 *
 * Selecting a monitor records the target and, when the window is already open,
 * moves it there immediately — so the control always has a visible effect.
 */
export function MonitorSelector() {
  const { monitors, monitorsState, monitorsError, targetMonitorKey, windowState, busy } =
    displayStore.use();

  return (
    <div className="flex flex-col gap-3">
      <MonitorList
        monitors={monitors}
        state={monitorsState}
        error={monitorsError}
        isDesktopRuntime={isDesktopRuntime()}
        selectedMonitorKey={targetMonitorKey}
        activeMonitorKey={windowState.monitorKey}
        disabled={busy}
        onSelect={(monitor) => void selectDisplayMonitor(monitor.key)}
        onRefresh={() => void refreshDisplay()}
      />

      <p className="text-[12px] leading-relaxed text-text-faint">
        {windowState.open
          ? "Selecting a monitor moves the open display window onto it straight away."
          : "The selected monitor is used the next time the display window opens."}
      </p>
    </div>
  );
}
