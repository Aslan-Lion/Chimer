import { cn } from "../../lib/cn";

export type StatusTone = "idle" | "running" | "paused" | "completed" | "stopped";

const TONE_CLASSES: Record<StatusTone, string> = {
  idle: "bg-text-faint",
  running: "bg-success",
  paused: "bg-warning",
  completed: "bg-accent",
  stopped: "bg-danger",
};

export interface StatusDotProps {
  tone: StatusTone;
  pulse?: boolean;
  className?: string;
}

export function StatusDot({ tone, pulse = false, className }: StatusDotProps) {
  return (
    <span className={cn("relative flex h-2 w-2 shrink-0", className)}>
      {pulse ? (
        <span
          className={cn(
            "absolute inline-flex h-full w-full animate-ping rounded-full opacity-60",
            TONE_CLASSES[tone],
          )}
        />
      ) : null}
      <span className={cn("relative inline-flex h-2 w-2 rounded-full", TONE_CLASSES[tone])} />
    </span>
  );
}
