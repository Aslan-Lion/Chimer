import { Copy, Play, Star, Trash2 } from "lucide-react";
import type { TimerPreset } from "../../types";
import { formatDuration } from "../../lib/time";
import { cn } from "../../lib/cn";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { IconButton } from "../ui/IconButton";

export interface PresetCardProps {
  preset: TimerPreset;
  messageName: string | null;
  onStart: () => void;
  onFavourite: () => void;
  onDuplicate: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

export function PresetCard({
  preset,
  messageName,
  onStart,
  onFavourite,
  onDuplicate,
  onEdit,
  onDelete,
}: PresetCardProps) {
  return (
    <Card className="flex flex-col gap-3 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-[14px] font-semibold text-text">{preset.name}</p>
          <p className="mt-0.5 line-clamp-2 text-[12px] text-text-muted">{preset.description}</p>
        </div>
        <IconButton
          label={preset.favourite ? "Remove from favourites" : "Add to favourites"}
          size="sm"
          onClick={onFavourite}
        >
          <Star
            size={15}
            className={cn(preset.favourite ? "text-warning" : "text-text-faint")}
            fill={preset.favourite ? "currentColor" : "none"}
          />
        </IconButton>
      </div>

      <div className="flex items-baseline gap-2">
        <span className="font-mono text-[22px] tabnum text-text">
          {formatDuration(preset.durationMs, "mm:ss")}
        </span>
        <span className="text-[11px] text-text-faint">
          {messageName ? `· ${messageName}` : "· no message"}
        </span>
      </div>

      <div className="flex items-center justify-between gap-2 border-t border-hairline pt-3">
        <div className="flex items-center gap-1">
          <IconButton label="Duplicate preset" size="sm" onClick={onDuplicate}>
            <Copy size={15} />
          </IconButton>
          <IconButton label="Delete preset" size="sm" onClick={onDelete}>
            <Trash2 size={15} />
          </IconButton>
          <Button variant="ghost" size="sm" onClick={onEdit}>
            Edit
          </Button>
        </div>
        <Button variant="primary" size="sm" icon={<Play size={14} />} onClick={onStart}>
          Start
        </Button>
      </div>
    </Card>
  );
}
