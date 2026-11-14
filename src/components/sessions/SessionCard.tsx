import { Copy, Play, Trash2 } from "lucide-react";
import type { SessionDefinition } from "../../types";
import { cn } from "../../lib/cn";
import { formatCompactDuration } from "../../lib/time";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { IconButton } from "../ui/IconButton";

export interface SessionCardProps {
  session: SessionDefinition;
  /** True while this very session is the one the engine is running. */
  running: boolean;
  onStart: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}

function totalDuration(session: SessionDefinition): number {
  return session.stages.reduce((sum, stage) => sum + stage.durationMs, 0);
}

export function SessionCard({
  session,
  running,
  onStart,
  onEdit,
  onDuplicate,
  onDelete,
}: SessionCardProps) {
  return (
    <Card className={cn("flex flex-col gap-3 p-4", running && "border-accent/60")}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-[14px] font-semibold text-text">{session.name}</p>
          <p className="mt-0.5 text-[11px] tracking-wide text-text-faint uppercase">
            {session.stages.length} stage{session.stages.length === 1 ? "" : "s"} ·{" "}
            {formatCompactDuration(totalDuration(session))}
          </p>
        </div>
        {running ? (
          <Badge tone="accent">Running</Badge>
        ) : (
          <IconButton label="Delete session" size="sm" onClick={onDelete}>
            <Trash2 size={15} />
          </IconButton>
        )}
      </div>

      <ol className="flex flex-col gap-1.5">
        {session.stages.slice(0, 4).map((stage, index) => (
          <li
            key={stage.id}
            className="flex items-center justify-between gap-3 rounded-lg border border-hairline bg-elevated px-3 py-1.5"
          >
            <span className="flex min-w-0 items-center gap-2">
              <span className="font-mono text-[11px] text-text-faint">{index + 1}</span>
              <span className="truncate text-[12px] text-text">{stage.name}</span>
            </span>
            <span className="shrink-0 text-[11px] text-text-faint">
              {formatCompactDuration(stage.durationMs)} · {stage.transition === "auto" ? "auto" : "manual"}
            </span>
          </li>
        ))}
        {session.stages.length > 4 ? (
          <li className="px-3 text-[11px] text-text-faint">
            +{session.stages.length - 4} more stage{session.stages.length - 4 === 1 ? "" : "s"}
          </li>
        ) : null}
      </ol>

      <div className="flex items-center justify-between gap-2 border-t border-hairline pt-3">
        <div className="flex items-center gap-1">
          <IconButton label="Duplicate session" size="sm" onClick={onDuplicate}>
            <Copy size={15} />
          </IconButton>
          <Button variant="ghost" size="sm" onClick={onEdit}>
            Edit
          </Button>
        </div>
        <Button
          variant="primary"
          size="sm"
          icon={<Play size={14} />}
          disabled={running || session.stages.length === 0}
          onClick={onStart}
        >
          Start session
        </Button>
      </div>
    </Card>
  );
}
