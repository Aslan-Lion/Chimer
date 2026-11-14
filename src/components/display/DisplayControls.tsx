import {
  ArrowRightLeft,
  Expand,
  Maximize,
  Minimize2,
  MonitorPlay,
  Pin,
  PinOff,
  Power,
  Presentation,
  RotateCcw,
} from "lucide-react";
import { Button } from "../ui/Button";
import { isDesktopRuntime } from "../../services/display";
import {
  closeDisplay,
  displayStore,
  moveToSecondMonitor,
  openDisplay,
  restoreDisplay,
  toggleDisplayAlwaysOnTop,
  toggleDisplayFullscreen,
  toggleDisplayMaximized,
  toggleDisplayPresentation,
} from "../../stores/displayStore";

/**
 * Native window commands for the presentation window.
 *
 * Every button maps to a real Tauri window operation executed in Rust — none of
 * them are decorative. Unavailable actions are disabled rather than hidden so
 * the current window state stays readable at a glance. Presentation mode is a
 * *mode of this window*, never another window: Rust remembers the window's
 * previous state and restores it on exit.
 */
export function DisplayControls({ compact = false }: { compact?: boolean }) {
  const { windowState, monitors, busy, error } = displayStore.use();

  const desktop = isDesktopRuntime();
  const open = windowState.open;
  const secondary = monitors.find((monitor) => !monitor.isPrimary) ?? null;
  const blocked = busy || !desktop;
  const size = compact ? "sm" : "md";

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <Button
          variant="primary"
          size={size}
          icon={<MonitorPlay size={16} />}
          disabled={blocked || open}
          onClick={() => void openDisplay()}
        >
          Open Display
        </Button>

        <Button
          variant="danger"
          size={size}
          icon={<Power size={16} />}
          disabled={blocked || !open}
          onClick={() => void closeDisplay()}
        >
          Close Display
        </Button>

        <Button
          size={size}
          icon={windowState.fullscreen ? <Minimize2 size={16} /> : <Expand size={16} />}
          disabled={blocked || !open}
          title="Switch the display window between windowed and fullscreen"
          onClick={() => void toggleDisplayFullscreen()}
        >
          {windowState.fullscreen ? "Exit Fullscreen" : "Toggle Fullscreen"}
        </Button>

        <Button
          size={size}
          icon={<Maximize size={16} />}
          disabled={blocked || !open}
          title="Maximise the display window, or return it to its previous size"
          onClick={() => void toggleDisplayMaximized()}
        >
          {windowState.maximized ? "Un-maximise" : "Maximise"}
        </Button>

        <Button
          size={size}
          icon={windowState.alwaysOnTop ? <PinOff size={16} /> : <Pin size={16} />}
          disabled={blocked || !open}
          title="Keep the display window above other windows"
          onClick={() => void toggleDisplayAlwaysOnTop()}
        >
          {windowState.alwaysOnTop ? "Unpin" : "Always on Top"}
        </Button>

        <Button
          variant={windowState.presentation ? "primary" : "secondary"}
          size={size}
          icon={<Presentation size={16} />}
          disabled={blocked || !open}
          title="Fullscreen, chrome-free and always on top; leaving it restores the previous window state"
          onClick={() => void toggleDisplayPresentation()}
        >
          {windowState.presentation ? "Exit Presentation" : "Presentation Mode"}
        </Button>

        <Button
          size={size}
          icon={<RotateCcw size={16} />}
          disabled={blocked || !open}
          title="Return the display window to a visible, resizable window"
          onClick={() => void restoreDisplay()}
        >
          Restore Window
        </Button>

        <Button
          size={size}
          icon={<ArrowRightLeft size={16} />}
          disabled={blocked || secondary === null}
          title={
            open
              ? "Move the display window onto the secondary monitor"
              : "Use the secondary monitor the next time the display window opens"
          }
          onClick={() => void moveToSecondMonitor()}
        >
          Move to Second Monitor
        </Button>
      </div>

      <p className="text-[12px] text-text-faint">
        {open
          ? `Window open${windowState.fullscreen ? " · fullscreen" : ""}${
              windowState.maximized ? " · maximised" : ""
            }${windowState.alwaysOnTop ? " · pinned" : ""}${
              windowState.presentation ? " · presentation mode" : ""
            }${windowState.monitorName ? ` · ${windowState.monitorName}` : ""}`
          : "Window closed — the counts still run, and reopening restores the current presentation."}
      </p>

      {!desktop ? (
        <p className="text-[12px] text-text-faint">
          These commands drive native Windows and are active in the desktop build.
        </p>
      ) : null}

      {error ? (
        <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
