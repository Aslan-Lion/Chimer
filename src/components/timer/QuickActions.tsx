import { Clock3, Expand, Minimize2, MonitorPlay } from "lucide-react";
import { setCreateTimerOpen } from "../../stores/uiStore";
import { displayStore, openDisplay, toggleDisplayFullscreen } from "../../stores/displayStore";
import { Button } from "../ui/Button";

/** Dashboard shortcuts. The display actions drive the real native window. */
export function QuickActions() {
  const { windowState, busy } = displayStore.use();
  const open = windowState.open;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-2">
        <Button
          variant="primary"
          icon={<Clock3 size={16} />}
          onClick={() => setCreateTimerOpen(true)}
        >
          Create Timer
        </Button>

        <Button
          icon={<MonitorPlay size={16} />}
          disabled={busy || open}
          title={open ? "The display window is already open" : "Open the native presentation window"}
          onClick={() => void openDisplay()}
        >
          Open Display
        </Button>

        <Button
          icon={windowState.fullscreen ? <Minimize2 size={16} /> : <Expand size={16} />}
          disabled={busy || !open}
          title={
            open
              ? "Switch the display window between windowed and fullscreen"
              : "Open the display window first"
          }
          onClick={() => void toggleDisplayFullscreen()}
        >
          {windowState.fullscreen ? "Exit Fullscreen" : "Fullscreen Display"}
        </Button>
      </div>
      <p className="text-[12px] text-text-faint">
        The display window is a separate native window — drag it to any monitor.
      </p>
    </div>
  );
}
