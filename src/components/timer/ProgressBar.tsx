import type { CSSProperties } from "react";
import { cn } from "../../lib/cn";

export interface ProgressBarProps {
  /** Elapsed fraction, 0 – 1, exactly as reported by the Rust engine. */
  progress: number;
  className?: string;
  /** Optional track / bar colours, so the presentation style can theme them. */
  trackStyle?: CSSProperties;
  barStyle?: CSSProperties;
}

export function ProgressBar({ progress, className, trackStyle, barStyle }: ProgressBarProps) {
  const value = Math.min(Math.max(progress, 0), 1);

  return (
    <div
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-[#26262c]", className)}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value * 100)}
      style={trackStyle}
    >
      <div
        className="h-full rounded-full bg-accent transition-[width] duration-300 ease-out"
        style={{ width: `${value * 100}%`, ...barStyle }}
      />
    </div>
  );
}
