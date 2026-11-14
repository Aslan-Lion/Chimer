import { cn } from "../../lib/cn";

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}

export function Toggle({ checked, onChange, label, description, disabled = false }: ToggleProps) {
  return (
    <label
      className={cn(
        "flex items-center justify-between gap-6 py-2.5",
        disabled && "opacity-50",
      )}
    >
      <span className="min-w-0">
        <span className="block text-[13px] font-medium text-text">{label}</span>
        {description ? (
          <span className="mt-0.5 block text-[12px] text-text-muted">{description}</span>
        ) : null}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cn(
          "relative h-6 w-11 shrink-0 rounded-full border transition-colors duration-200",
          "disabled:pointer-events-none",
          checked ? "border-accent/60 bg-accent/80" : "border-border bg-elevated",
        )}
      >
        <span
          className={cn(
            "absolute top-0.5 h-4.5 w-4.5 rounded-full bg-white transition-all duration-200",
            checked ? "left-[22px]" : "left-0.5",
          )}
          style={{ height: 18, width: 18 }}
        />
      </button>
    </label>
  );
}
