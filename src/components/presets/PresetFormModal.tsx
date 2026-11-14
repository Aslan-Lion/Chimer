import { useEffect, useState } from "react";
import type { TimerPreset } from "../../types";
import { addPreset, updatePreset } from "../../stores/libraryStore";
import { libraryStore } from "../../stores/libraryStore";
import type { DurationParts } from "../../lib/time";
import { fromDurationParts, toDurationParts } from "../../lib/time";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { DurationInput } from "../timer/DurationInput";

const SELECT_CLASS =
  "h-11 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

export interface PresetFormModalProps {
  open: boolean;
  /** `null` creates a new preset, otherwise the preset is edited in place. */
  preset: TimerPreset | null;
  onClose: () => void;
}

export function PresetFormModal({ open, preset, onClose }: PresetFormModalProps) {
  const { messages } = libraryStore.use();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [duration, setDuration] = useState<DurationParts>({ hours: 0, minutes: 25, seconds: 0 });
  const [messageId, setMessageId] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setName(preset?.name ?? "");
    setDescription(preset?.description ?? "");
    setDuration(
      preset ? toDurationParts(preset.durationMs) : { hours: 0, minutes: 25, seconds: 0 },
    );
    setMessageId(preset?.defaultMessageId ?? "");
    setError(null);
  }, [open, preset]);

  const totalMs = fromDurationParts(duration);

  const handleSave = () => {
    if (name.trim().length === 0) {
      setError("Give the preset a name.");
      return;
    }
    if (totalMs <= 0) {
      setError("Duration must be greater than zero.");
      return;
    }

    if (preset) {
      updatePreset(preset.id, {
        name: name.trim(),
        description: description.trim(),
        durationMs: totalMs,
        defaultMessageId: messageId || null,
      });
    } else {
      addPreset({
        name: name.trim(),
        description: description.trim(),
        durationMs: totalMs,
        defaultMessageId: messageId || null,
        favourite: false,
      });
    }
    onClose();
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={preset ? "Edit preset" : "New preset"}
      description="Presets store a name, duration and an optional default message."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={handleSave}>
            {preset ? "Save changes" : "Create preset"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Preset name
          </span>
          <input
            type="text"
            value={name}
            autoFocus
            onChange={(event) => setName(event.currentTarget.value)}
            className="h-11 w-full rounded-xl border border-border bg-elevated px-3 text-sm text-text outline-none focus:border-accent/70"
          />
        </label>

        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Description
          </span>
          <input
            type="text"
            value={description}
            placeholder="Optional note shown on the preset card"
            onChange={(event) => setDescription(event.currentTarget.value)}
            className="h-11 w-full rounded-xl border border-border bg-elevated px-3 text-sm text-text outline-none placeholder:text-text-faint focus:border-accent/70"
          />
        </label>

        <DurationInput value={duration} onChange={setDuration} onEnter={handleSave} />

        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Default message
          </span>
          <select
            value={messageId}
            onChange={(event) => setMessageId(event.currentTarget.value)}
            className={SELECT_CLASS}
          >
            <option value="">No message</option>
            {messages.map((message) => (
              <option key={message.id} value={message.id}>
                {message.name}
              </option>
            ))}
          </select>
        </label>

        {error ? (
          <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
