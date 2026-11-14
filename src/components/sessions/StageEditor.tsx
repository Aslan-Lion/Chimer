import { useState } from "react";
import { ChevronDown, ChevronUp, Copy, Eye, Trash2 } from "lucide-react";
import type {
  MessagePreset,
  SessionStage,
  StageMessage,
  StageTransition,
  TimerPreset,
} from "../../types";
import type { DurationParts } from "../../lib/time";
import { formatCompactDuration, fromDurationParts, toDurationParts } from "../../lib/time";
import { createId } from "../../lib/id";
import { Button } from "../ui/Button";
import { IconButton } from "../ui/IconButton";
import { NumberField } from "../ui/NumberField";
import { Toggle } from "../ui/Toggle";
import { DurationInput } from "../timer/DurationInput";

const SELECT_CLASS =
  "h-11 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

const FIELD_CLASS =
  "h-11 w-full rounded-xl border border-border bg-elevated px-3 text-sm text-text outline-none transition-colors duration-150 placeholder:text-text-faint focus:border-accent/70";

const LABEL_CLASS = "text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase";

/**
 * Editable form of a stage. It deliberately differs from `SessionStage` in one
 * place — the warning thresholds are held as raw text so a partially typed list
 * never has to be parsed mid-keystroke.
 */
export interface StageDraft {
  id: string;
  name: string;
  duration: DurationParts;
  /** Library message id, or `""` for no message. */
  messageId: string;
  /** Comma-separated list of warning thresholds, in seconds. */
  warningText: string;
  soundEnabled: boolean;
  transition: StageTransition;
  transitionCountdownSeconds: number;
}

export function emptyStageDraft(index: number): StageDraft {
  return {
    id: createId(),
    name: `Stage ${index + 1}`,
    duration: { hours: 0, minutes: 25, seconds: 0 },
    messageId: "",
    warningText: "",
    soundEnabled: true,
    transition: "auto",
    transitionCountdownSeconds: 0,
  };
}

/** Project a library message onto the resolved payload a stage carries. */
export function resolveStageMessage(message: MessagePreset): StageMessage {
  return {
    id: message.id,
    name: message.name,
    text: message.text,
    fontSize: message.fontSize,
    color: message.color,
    backgroundColor: message.backgroundColor,
    position: message.position,
    font: message.font,
    animation: message.animation,
    durationMs: message.durationMs,
  };
}

export function stageDraftFromStage(stage: SessionStage, messages: MessagePreset[]): StageDraft {
  const messageId = stage.message?.id ?? "";
  const known = messageId !== "" && messages.some((message) => message.id === messageId);
  return {
    id: stage.id,
    name: stage.name,
    duration: toDurationParts(stage.durationMs),
    messageId: known ? messageId : "",
    warningText: stage.warningIntervalsMs
      .map((value) => Math.round(value / 1000))
      .join(", "),
    soundEnabled: stage.soundEnabled,
    transition: stage.transition,
    transitionCountdownSeconds: Math.round((stage.transitionCountdownMs ?? 0) / 1000),
  };
}

export interface ParsedStage {
  stage: SessionStage;
  error: string | null;
}

/**
 * Turns a draft back into the wire shape. Any problem is reported rather than
 * silently coerced: a session is validated before it is saved or started.
 */
export function stageFromDraft(draft: StageDraft, messages: MessagePreset[]): ParsedStage {
  const durationMs = fromDurationParts(draft.duration);
  if (draft.name.trim().length === 0) {
    return { stage: EMPTY_STAGE(draft), error: "Every stage needs a name." };
  }
  if (durationMs <= 0) {
    return { stage: EMPTY_STAGE(draft), error: `"${draft.name}" needs a duration greater than zero.` };
  }

  const thresholds: number[] = [];
  const tokens = draft.warningText
    .split(",")
    .map((token) => token.trim())
    .filter((token) => token.length > 0);
  for (const token of tokens) {
    if (!/^\d+$/.test(token)) {
      return { stage: EMPTY_STAGE(draft), error: `"${token}" is not a whole number of seconds.` };
    }
    const seconds = Number(token);
    if (seconds <= 0) {
      return { stage: EMPTY_STAGE(draft), error: "Warning intervals must be greater than zero." };
    }
    if (seconds * 1000 >= durationMs) {
      return {
        stage: EMPTY_STAGE(draft),
        error: `The warning at ${seconds}s is longer than "${draft.name}".`,
      };
    }
    thresholds.push(seconds * 1000);
  }

  const message = messages.find((entry) => entry.id === draft.messageId);
  const countdownSeconds = draft.transitionCountdownSeconds;

  return {
    stage: {
      id: draft.id,
      name: draft.name.trim(),
      durationMs,
      message: message ? resolveStageMessage(message) : null,
      warningIntervalsMs: thresholds,
      soundEnabled: draft.soundEnabled,
      transition: draft.transition,
      transitionCountdownMs:
        draft.transition === "auto" && countdownSeconds > 0 ? countdownSeconds * 1000 : null,
    },
    error: null,
  };
}

function EMPTY_STAGE(draft: StageDraft): SessionStage {
  return {
    id: draft.id,
    name: draft.name,
    durationMs: 0,
    message: null,
    warningIntervalsMs: [],
    soundEnabled: false,
    transition: "auto",
    transitionCountdownMs: null,
  };
}

export interface StageEditorProps {
  stage: StageDraft;
  index: number;
  total: number;
  messages: MessagePreset[];
  /** Timer presets the stage can be seeded from. */
  presets: TimerPreset[];
  onChange: (patch: Partial<StageDraft>) => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onPreview: () => void;
}

export function StageEditor({
  stage,
  index,
  total,
  messages,
  presets,
  onChange,
  onMoveUp,
  onMoveDown,
  onDuplicate,
  onDelete,
  onPreview,
}: StageEditorProps) {
  const durationMs = fromDurationParts(stage.duration);
  const [presetId, setPresetId] = useState("");

  /** Seed the stage from a stored timer preset; every field stays editable. */
  const applyPreset = (id: string) => {
    setPresetId(id);
    const preset = presets.find((entry) => entry.id === id);
    if (!preset) return;
    onChange({
      name: preset.name,
      duration: toDurationParts(preset.durationMs),
      messageId: preset.defaultMessageId ?? "",
    });
  };

  return (
    <div className="rounded-[14px] border border-border bg-[#141417] p-4">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border bg-elevated font-mono text-[12px] text-text-muted">
            {index + 1}
          </span>
          <p className="truncate text-[13px] font-semibold text-text">
            {stage.name.trim() || "Untitled stage"}
          </p>
          <span className="shrink-0 text-[11px] text-text-faint">
            · {formatCompactDuration(durationMs)}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <IconButton
            label="Move stage up"
            size="sm"
            disabled={index === 0}
            onClick={onMoveUp}
          >
            <ChevronUp size={15} />
          </IconButton>
          <IconButton
            label="Move stage down"
            size="sm"
            disabled={index === total - 1}
            onClick={onMoveDown}
          >
            <ChevronDown size={15} />
          </IconButton>
          <IconButton label="Preview this stage" size="sm" onClick={onPreview}>
            <Eye size={15} />
          </IconButton>
          <IconButton label="Duplicate stage" size="sm" onClick={onDuplicate}>
            <Copy size={15} />
          </IconButton>
          <IconButton label="Delete stage" size="sm" onClick={onDelete}>
            <Trash2 size={15} />
          </IconButton>
        </div>
      </div>

      <div className="flex flex-col gap-4">
        <label className="flex flex-col gap-1.5">
          <span className={LABEL_CLASS}>Stage name</span>
          <input
            type="text"
            value={stage.name}
            placeholder="e.g. Written paper"
            onChange={(event) => onChange({ name: event.currentTarget.value })}
            className={FIELD_CLASS}
          />
        </label>

        <label className="flex flex-col gap-1.5">
          <span className={LABEL_CLASS}>Start from a timer preset</span>
          <select
            value={presetId}
            onChange={(event) => applyPreset(event.currentTarget.value)}
            className={SELECT_CLASS}
          >
            <option value="">Custom stage</option>
            {presets.map((preset) => (
              <option key={preset.id} value={preset.id}>
                {preset.name}
              </option>
            ))}
          </select>
        </label>

        <DurationInput
          value={stage.duration}
          onChange={(duration) => onChange({ duration })}
        />

        <label className="flex flex-col gap-1.5">
          <span className={LABEL_CLASS}>Message preset</span>
          <select
            value={stage.messageId}
            onChange={(event) => onChange({ messageId: event.currentTarget.value })}
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

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label className="flex flex-col gap-1.5">
            <span className={LABEL_CLASS}>Warning intervals</span>
            <input
              type="text"
              value={stage.warningText}
              placeholder="e.g. 600, 300, 60"
              onChange={(event) => onChange({ warningText: event.currentTarget.value })}
              className={FIELD_CLASS}
            />
          </label>

          <div className="flex flex-col gap-1.5">
            <span className={LABEL_CLASS}>Transition</span>
            <div className="flex items-center gap-3">
              <select
                value={stage.transition}
                onChange={(event) =>
                  onChange({ transition: event.currentTarget.value as StageTransition })
                }
                className={SELECT_CLASS}
              >
                <option value="auto">Automatic</option>
                <option value="manual">Manual</option>
              </select>
              {stage.transition === "auto" ? (
                <NumberField
                  label="Countdown"
                  value={stage.transitionCountdownSeconds}
                  max={600}
                  suffix="s"
                  onChange={(transitionCountdownSeconds) => onChange({ transitionCountdownSeconds })}
                />
              ) : null}
            </div>
          </div>
        </div>

        <p className="text-[12px] text-text-faint">
          Warning intervals are seconds remaining, separated by commas.
          {stage.transition === "auto"
            ? " The next stage starts automatically after the countdown."
            : " The next stage waits until you start it."}
        </p>

        <div className="rounded-xl border border-border bg-card px-4 py-1">
          <Toggle
            checked={stage.soundEnabled}
            onChange={(soundEnabled) => onChange({ soundEnabled })}
            label="Sound notifications"
            description="Play the stage's message sound when it starts and at each warning."
          />
        </div>

        <div className="flex justify-end">
          <Button variant="ghost" size="sm" onClick={onDelete}>
            Remove stage
          </Button>
        </div>
      </div>
    </div>
  );
}
