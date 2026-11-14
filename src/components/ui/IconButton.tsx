import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** Accessible name, also used as the native tooltip. */
  label: string;
  size?: "sm" | "md";
  active?: boolean;
  children: ReactNode;
}

export function IconButton({
  label,
  size = "md",
  active = false,
  className,
  children,
  ...rest
}: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex items-center justify-center rounded-lg border border-transparent text-text-muted transition-colors duration-150",
        "hover:bg-[#1c1c20] hover:text-text",
        "disabled:pointer-events-none disabled:opacity-40",
        active && "border-border bg-elevated text-text",
        size === "sm" ? "h-8 w-8" : "h-9 w-9",
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}
