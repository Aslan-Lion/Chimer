import { useMemo, useState } from "react";
import { addPreset, addTimer, libraryStore } from "../../stores/libraryStore";
import { clearTimerError, startNewTimer } from "../../stores/timerStore";
import { navigateTo, setCreateTimerOpen, uiStore } from "../../stores/uiStore";
import type { DurationParts } from "../../lib/time";
import { fromDurationParts, toDurationParts } from "../../lib/time";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Toggle } from "../ui/Toggle";
import { DurationInput } from "./DurationInput";

const SELECT_CLASS =
  "h-11 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

export function CreateTimerModal() {
  const { createTimerOpen } = uiStore.use();
  const { messages, presets } = libraryStore.use();

  const [presetId, setPresetId] = useState("");
  const [name, setName] = useState("");
  const [duration, setDuration] = useState<DurationParts>({ hours: 0, minutes: 25, seconds: 0 });
  const [messageId, setMessageId] = useState("");
  const [soundEnabled, setSoundEnabled] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const totalMs = useMemo(() => fromDurationParts(duration), [duration]);

  const reset = () => {
    setPresetId("");
    setName("");
    setDuration({ hours: 0, minutes: 25, seconds: 0 });
    setMessageId("");
    setSoundEnabled(true);
    setError(null);
  };

  /**
   * A stored timer preset is a starting point, not a hidden binding: choosing
   * one copies its name, duration and default message into the form so they can
   * still be adjusted before the countdown starts.
   */
  const applyPreset = (id: string) => {
    setPresetId(id);
    const preset = presets.find((entry) => entry.id === id);
    if (!preset) return;
    setName(preset.name);
    setDuration(toDurationParts(preset.durationMs));
    setMessageId(preset.defaultMessageId ?? "");
    setError(null);
  };

  const close = () => {
    reset();
    setCreateTimerOpen(false);
  };

  const validate = (): string | null => {
    if (name.trim().length === 0) return "Give the timer a name before continuing.";
    if (totalMs <= 0) return "Duration must be greater than zero.";
    return null;
  };

  const handleStart = async () => {
    const problem = validate();
    if (problem) {
      setError(problem);
      return;
    }
    // The engine owns the countdown; a rejected configuration is reported here
    // rather than silently ignored.
    const started = await startNewTimer(name.trim(), totalMs);
    if (!started) {
      setError(clearTimerError() ?? "The timer engine rejected this timer.");
      return;
    }
    // The library only keeps the definition so the timer can be started again.
    addTimer({
      name: name.trim(),
      durationMs: totalMs,
      messageId: messageId || null,
      soundEnabled,
    });
    close();
    navigateTo("dashboard");
  };

  const handleSavePreset = () => {
    const problem = validate();
    if (problem) {
      setError(problem);
      return;
    }
    addPreset({
      name: name.trim(),
      durationMs: totalMs,
      description: "Created from the timer dialog.",
      defaultMessageId: messageId || null,
      favourite: false,
    });
    close();
    navigateTo("presets");
  };

  return (
    <Modal
      open={createTimerOpen}
      onClose={close}
      title="Create timer"
      description="Configure a countdown and arm it in the workspace."
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button onClick={handleSavePreset}>Save as Preset</Button>
          <Button variant="primary" onClick={handleStart}>
            Start
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Start from a preset
          </span>
          <select
            value={presetId}
            onChange={(event) => applyPreset(event.currentTarget.value)}
            className={SELECT_CLASS}
          >
            <option value="">Blank timer</option>
            {presets.map((preset) => (
              <option key={preset.id} value={preset.id}>
                {preset.name}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Timer name
          </span>
          <input
            type="text"
            value={name}
            autoFocus
            placeholder="e.g. Morning examination"
            onChange={(event) => setName(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") handleStart();
            }}
            className="h-11 w-full rounded-xl border border-border bg-elevated px-3 text-sm text-text outline-none transition-colors duration-150 placeholder:text-text-faint focus:border-accent/70"
          />
        </label>

        <DurationInput value={duration} onChange={setDuration} onEnter={handleStart} />

        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Message preset
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

        <div className="rounded-xl border border-border bg-[#141417] px-4 py-1">
          <Toggle
            checked={soundEnabled}
            onChange={setSoundEnabled}
            label="Sound notification"
            description="Play the completion sound when the countdown reaches zero."
          />
        </div>

        {error ? (
          <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
