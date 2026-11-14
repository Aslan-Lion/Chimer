import type { DurationParts } from "../../lib/time";
import { NumberField } from "../ui/NumberField";

export interface DurationInputProps {
  value: DurationParts;
  onChange: (next: DurationParts) => void;
  onEnter?: () => void;
}

/** Hours / minutes / seconds entry with keyboard-friendly tab order. */
export function DurationInput({ value, onChange, onEnter }: DurationInputProps) {
  return (
    <div className="flex items-end gap-3">
      <NumberField
        label="Hours"
        value={value.hours}
        max={99}
        autoFocus
        onEnter={onEnter}
        onChange={(hours) => onChange({ ...value, hours })}
      />
      <NumberField
        label="Minutes"
        value={value.minutes}
        max={59}
        onEnter={onEnter}
        onChange={(minutes) => onChange({ ...value, minutes })}
      />
      <NumberField
        label="Seconds"
        value={value.seconds}
        max={59}
        onEnter={onEnter}
        onChange={(seconds) => onChange({ ...value, seconds })}
      />
    </div>
  );
}
