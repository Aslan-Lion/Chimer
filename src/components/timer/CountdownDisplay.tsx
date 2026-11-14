import type { TimerFormat } from "../../lib/time";
import { formatDuration } from "../../lib/time";
import { cn } from "../../lib/cn";

export interface CountdownDisplayProps {
  ms: number;
  format?: TimerFormat;
  className?: string;
}

/** Large tabular-figure countdown shared by the control and display windows. */
export function CountdownDisplay({ ms, format = "hh:mm:ss", className }: CountdownDisplayProps) {
  return (
    <span
      className={cn("font-mono tabnum leading-none text-text", className)}
      aria-live="off"
    >
      {formatDuration(ms, format)}
    </span>
  );
}
