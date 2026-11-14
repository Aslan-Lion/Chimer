import { Monitor, RefreshCw } from "lucide-react";
import type { MonitorInfo } from "../../types";
import type { MonitorLoadState } from "../../hooks/useMonitors";
import { cn } from "../../lib/cn";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";

export interface MonitorListProps {
  monitors: MonitorInfo[];
  state: MonitorLoadState;
  error: string | null;
  isDesktopRuntime: boolean;
  /** Monitor the presentation window will open on / has been moved to. */
  selectedMonitorKey: string | null;
  /** Monitor the presentation window currently occupies, when it is open. */
  activeMonitorKey: string | null;
  disabled?: boolean;
  onSelect: (monitor: MonitorInfo) => void;
  onRefresh: () => void;
}

function monitorLabel(monitor: MonitorInfo, index: number): string {
  return monitor.name ?? `Display ${index + 1}`;
}

export function MonitorList({
  monitors,
  state,
  error,
  isDesktopRuntime,
  selectedMonitorKey,
  activeMonitorKey,
  disabled = false,
  onSelect,
  onRefresh,
}: MonitorListProps) {
  if (!isDesktopRuntime) {
    return (
      <p className="rounded-xl border border-dashed border-border px-4 py-6 text-center text-[13px] text-text-muted">
        Monitor detection needs the desktop runtime. Launch the app with <code>npm run tauri dev</code>{" "}
        to enumerate connected displays.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-[12px] text-text-faint">
          {state === "ready" ? `${monitors.length} monitor(s) detected` : "Scanning…"}
        </p>
        <Button size="sm" icon={<RefreshCw size={14} />} onClick={onRefresh} disabled={disabled}>
          Refresh
        </Button>
      </div>

      {error ? (
        <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
          {error}
        </p>
      ) : null}

      {state === "ready" && monitors.length === 1 ? (
        <p className="rounded-lg border border-[#4a4120] bg-[#241f10] px-3 py-2 text-[12px] text-warning">
          Only one display is connected. The presentation window opens on the primary monitor until
          a second display is attached.
        </p>
      ) : null}

      <div className="flex flex-col gap-2">
        {monitors.map((monitor, index) => {
          const selected = monitor.key === selectedMonitorKey;
          const active = monitor.key === activeMonitorKey;

          return (
            <button
              key={monitor.key}
              type="button"
              disabled={disabled}
              onClick={() => onSelect(monitor)}
              className={cn(
                "flex items-center justify-between gap-4 rounded-xl border px-4 py-3 text-left transition-colors duration-150",
                "disabled:pointer-events-none disabled:opacity-50",
                selected
                  ? "border-accent/60 bg-[#191c2b]"
                  : "border-border bg-elevated hover:border-border-strong",
              )}
            >
              <span className="flex min-w-0 items-center gap-3">
                <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-text-muted">
                  <Monitor size={16} />
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-[13px] font-medium text-text">
                    {monitorLabel(monitor, index)}
                  </span>
                  <span className="block text-[11px] text-text-faint">
                    {monitor.width} × {monitor.height} · {Math.round(monitor.scaleFactor * 100)}% ·
                    origin {monitor.x},{monitor.y}
                  </span>
                </span>
              </span>

              <span className="flex shrink-0 items-center gap-2">
                {monitor.isPrimary ? <Badge tone="accent">Primary</Badge> : null}
                {monitor.isCurrent ? <Badge tone="success">Control window</Badge> : null}
                {active ? <Badge tone="success">Display window</Badge> : null}
                {selected ? <Badge tone="accent">Selected</Badge> : null}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
