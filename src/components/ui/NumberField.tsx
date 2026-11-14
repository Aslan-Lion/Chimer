import { cn } from "../../lib/cn";

export interface NumberFieldProps {
  label: string;
  value: number;
  onChange: (value: number) => void;
  max?: number;
  suffix?: string;
  autoFocus?: boolean;
  onEnter?: () => void;
}

/**
 * Numeric text field used for duration entry. Digits are filtered on input so
 * invalid or negative values can never reach the timer definition.
 */
export function NumberField({
  label,
  value,
  onChange,
  max = 999,
  suffix,
  autoFocus = false,
  onEnter,
}: NumberFieldProps) {
  return (
    <label className="flex min-w-0 flex-1 flex-col gap-1.5">
      <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
        {label}
      </span>
      <span className="relative flex items-center">
        <input
          type="text"
          inputMode="numeric"
          autoFocus={autoFocus}
          value={value}
          onFocus={(event) => event.currentTarget.select()}
          onChange={(event) => {
            const digits = event.currentTarget.value.replace(/[^\d]/g, "");
            const parsed = digits === "" ? 0 : Number(digits);
            onChange(Math.min(parsed, max));
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && onEnter) onEnter();
          }}
          className={cn(
            "h-11 w-full rounded-xl border border-border bg-elevated px-3 font-mono text-[15px] tabnum text-text",
            "transition-colors duration-150 outline-none focus:border-accent/70 focus:bg-[#1d1d22]",
          )}
        />
        {suffix ? (
          <span className="pointer-events-none absolute right-3 text-[11px] text-text-faint">
            {suffix}
          </span>
        ) : null}
      </span>
    </label>
  );
}
