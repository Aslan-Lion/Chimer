import { MonitorPlay } from "lucide-react";
import { DisplayControls } from "./DisplayControls";
import { Badge } from "../ui/Badge";
import { Card, CardHeader } from "../ui/Card";
import { StatusDot } from "../ui/StatusDot";
import { displayStore } from "../../stores/displayStore";

interface StatusRowProps {
  label: string;
  value: string;
}

function StatusRow({ label, value }: StatusRowProps) {
  return (
    <div className="flex items-center justify-between border-b border-hairline py-2.5 last:border-b-0">
      <span className="text-[13px] text-text-muted">{label}</span>
      <span className="truncate pl-4 font-mono text-[12px] text-text">{value}</span>
    </div>
  );
}

/**
 * Live state of the native presentation window plus the commands that drive it.
 * Every value is read from the real window registry through Rust.
 */
export function DisplayWindowStatusCard() {
  const { windowState, monitors } = displayStore.use();

  const open = windowState.open;
  const mode = !open
    ? "—"
    : windowState.presentation
      ? "Presentation"
      : windowState.fullscreen
        ? "Fullscreen"
        : windowState.maximized
          ? "Maximised"
          : "Windowed";

  const monitorLabel = open
    ? (windowState.monitorName ?? windowState.monitorKey ?? "Unknown display")
    : "Not assigned";

  return (
    <Card className="flex flex-col">
      <CardHeader
        icon={<MonitorPlay size={16} />}
        eyebrow="Display"
        title="Display window"
        action={
          open ? (
            <Badge tone="success">
              <StatusDot tone="running" pulse />
              Open
            </Badge>
          ) : (
            <Badge tone="neutral">
              <StatusDot tone="idle" />
              Closed
            </Badge>
          )
        }
      />

      <div className="px-5 pb-4">
        <StatusRow label="Status" value={open ? "Open" : "Closed"} />
        <StatusRow label="Monitor" value={monitorLabel} />
        <StatusRow label="Mode" value={mode} />
        <StatusRow label="Monitors detected" value={String(monitors.length)} />
      </div>

      <div className="border-t border-hairline px-5 py-4">
        <DisplayControls compact />
      </div>
    </Card>
  );
}
