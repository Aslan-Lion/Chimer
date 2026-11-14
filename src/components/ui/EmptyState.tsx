import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

export interface EmptyStateProps {
  icon: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-3 rounded-[14px] border border-dashed border-border px-6 py-12 text-center",
        className,
      )}
    >
      <span className="flex h-11 w-11 items-center justify-center rounded-xl border border-border bg-elevated text-text-muted">
        {icon}
      </span>
      <div>
        <p className="text-sm font-semibold text-text">{title}</p>
        <p className="mx-auto mt-1 max-w-sm text-[13px] leading-relaxed text-text-muted">
          {description}
        </p>
      </div>
      {action}
    </div>
  );
}
