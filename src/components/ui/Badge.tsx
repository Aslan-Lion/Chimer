import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger";

const TONE_CLASSES: Record<BadgeTone, string> = {
  neutral: "border-border bg-elevated text-text-muted",
  accent: "border-[#3a4270] bg-[#1b1e33] text-accent",
  success: "border-[#1f4531] bg-[#10231a] text-success",
  warning: "border-[#4a4120] bg-[#241f10] text-warning",
  danger: "border-[#4a2020] bg-[#2a1414] text-danger",
};

export interface BadgeProps {
  tone?: BadgeTone;
  children: ReactNode;
  className?: string;
}

export function Badge({ tone = "neutral", children, className }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] font-medium tracking-wide",
        TONE_CLASSES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
