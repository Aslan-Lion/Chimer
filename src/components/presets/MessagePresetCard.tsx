import { Copy, Eye, Trash2 } from "lucide-react";
import type { MessagePreset } from "../../types";
import { cn } from "../../lib/cn";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { IconButton } from "../ui/IconButton";
import { MessageSurface } from "../display/MessageSurface";

export interface MessagePresetCardProps {
  message: MessagePreset;
  previewing: boolean;
  onPreview: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}

export function MessagePresetCard({
  message,
  previewing,
  onPreview,
  onEdit,
  onDuplicate,
  onDelete,
}: MessagePresetCardProps) {
  return (
    <Card className={cn("flex flex-col gap-3 p-4", previewing && "border-accent/60")}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-[14px] font-semibold text-text">{message.name}</p>
          <p className="mt-0.5 text-[11px] tracking-wide text-text-faint uppercase">
            {message.position} · {Math.round(message.durationMs / 1000)}s · {message.fontSize}px
            {message.sound !== "none" ? ` · ${message.sound}` : ""}
          </p>
        </div>
        <div className="flex items-center gap-1">
          <IconButton
            label={previewing ? "Hide preview" : "Preview on the display window"}
            size="sm"
            active={previewing}
            onClick={onPreview}
          >
            <Eye size={15} />
          </IconButton>
          <IconButton label="Duplicate message" size="sm" onClick={onDuplicate}>
            <Copy size={15} />
          </IconButton>
          <IconButton label="Delete message" size="sm" onClick={onDelete}>
            <Trash2 size={15} />
          </IconButton>
        </div>
      </div>

      <div className="flex min-h-[74px] items-center justify-center rounded-xl border border-hairline bg-display px-3 py-3">
        <MessageSurface
          message={message}
          className="text-center"
          style={{ fontSize: Math.max(13, Math.round(message.fontSize / 6)) }}
        />
      </div>

      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] tracking-wide text-text-faint uppercase">
          {message.font} · {message.animation}
        </span>
        <Button variant="ghost" size="sm" onClick={onEdit}>
          Edit
        </Button>
      </div>
    </Card>
  );
}
