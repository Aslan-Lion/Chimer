import { useEffect, useState } from "react";
import { Play, RotateCcw } from "lucide-react";
import type {
  MessageAnimation,
  MessageFont,
  MessagePosition,
  MessagePreset,
  MessageSound,
} from "../../types";
import { addMessage, updateMessage } from "../../stores/libraryStore";
import { MESSAGE_ANIMATIONS, MESSAGE_FONTS } from "../../lib/message";
import { SOUND_OPTIONS, playSound } from "../../lib/sound";
import { cn } from "../../lib/cn";
import { Button } from "../ui/Button";
import { IconButton } from "../ui/IconButton";
import { Modal } from "../ui/Modal";
import { NumberField } from "../ui/NumberField";
import { MessageSurface } from "../display/MessageSurface";

const SELECT_CLASS =
  "h-11 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

const FIELD_CLASS =
  "h-11 w-full rounded-xl border border-border bg-elevated px-3 text-sm text-text outline-none transition-colors duration-150 placeholder:text-text-faint focus:border-accent/70";

const LABEL_CLASS = "text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase";

const POSITIONS: MessagePosition[] = ["top", "center", "bottom"];

/** Row alignment of the preview surface, mirroring the display window. */
const POSITION_ALIGN: Record<MessagePosition, string> = {
  top: "items-start",
  center: "items-center",
  bottom: "items-end",
};

export interface MessageFormModalProps {
  open: boolean;
  /** `null` creates a new message, otherwise the message is edited in place. */
  message: MessagePreset | null;
  onClose: () => void;
}

interface Draft {
  name: string;
  text: string;
  fontSize: number;
  color: string;
  backgroundEnabled: boolean;
  backgroundColor: string;
  position: MessagePosition;
  font: MessageFont;
  animation: MessageAnimation;
  durationSeconds: number;
  sound: MessageSound;
}

const EMPTY: Draft = {
  name: "",
  text: "",
  fontSize: 104,
  color: "#F4F4F5",
  backgroundEnabled: false,
  backgroundColor: "#14202A",
  position: "bottom",
  font: "sans",
  animation: "fade",
  durationSeconds: 8,
  sound: "none",
};

/**
 * Message Library editor.
 *
 * A two-column dialog: the controls on the left, a live preview on the right.
 * Every field is a controlled React value — no DOM query is ever used to read
 * an input — and the value is always taken from `event.currentTarget`
 * synchronously, because React clears that property once the handler returns.
 */
export function MessageFormModal({ open, message, onClose }: MessageFormModalProps) {
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [error, setError] = useState<string | null>(null);
  // Bumped to replay the entrance animation in the preview.
  const [replay, setReplay] = useState(0);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setReplay((value) => value + 1);
    if (!message) {
      setDraft(EMPTY);
      return;
    }
    setDraft({
      name: message.name,
      text: message.text,
      fontSize: message.fontSize,
      color: message.color,
      backgroundEnabled: message.backgroundColor !== null,
      backgroundColor: message.backgroundColor ?? "#14202A",
      position: message.position,
      font: message.font,
      animation: message.animation,
      durationSeconds: Math.round(message.durationMs / 1000),
      sound: message.sound,
    });
  }, [open, message]);

  const patch = (change: Partial<Draft>) =>
    setDraft((previous) => ({ ...previous, ...change }));

  const handleSave = () => {
    if (draft.name.trim().length === 0) {
      setError("Give the message a name.");
      return;
    }
    if (draft.text.trim().length === 0) {
      setError("The message needs some text to display.");
      return;
    }
    if (draft.fontSize <= 0) {
      setError("Font size must be greater than zero.");
      return;
    }
    if (draft.durationSeconds <= 0) {
      setError("Display duration must be greater than zero.");
      return;
    }

    const payload = {
      name: draft.name.trim(),
      text: draft.text,
      fontSize: draft.fontSize,
      color: draft.color,
      backgroundColor: draft.backgroundEnabled ? draft.backgroundColor : null,
      position: draft.position,
      font: draft.font,
      animation: draft.animation,
      durationMs: draft.durationSeconds * 1000,
      sound: draft.sound,
    };

    if (message) {
      updateMessage(message.id, payload);
    } else {
      addMessage(payload);
    }
    onClose();
  };

  const previewScale = Math.max(16, Math.round(draft.fontSize / 4.5));
  const preview = {
    text: draft.text,
    color: draft.color,
    backgroundColor: draft.backgroundEnabled ? draft.backgroundColor : null,
    font: draft.font,
    animation: draft.animation,
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={message ? "Edit message" : "New message"}
      description="Messages are shown on the presentation display during a countdown."
      width={940}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={handleSave}>
            {message ? "Save changes" : "Create message"}
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        {/* ------------------------------------------------- control column */}
        <div className="flex min-w-0 flex-col gap-4">
          <label className="flex flex-col gap-1.5">
            <span className={LABEL_CLASS}>Message name</span>
            <input
              type="text"
              autoFocus
              value={draft.name}
              placeholder="e.g. Five minutes remaining"
              onChange={(event) => patch({ name: event.currentTarget.value })}
              className={cn(FIELD_CLASS, "placeholder:text-text-faint")}
            />
          </label>

          <label className="flex flex-col gap-1.5">
            <span className={LABEL_CLASS}>Message content</span>
            <textarea
              value={draft.text}
              rows={3}
              placeholder="Text shown on the display window"
              onChange={(event) => patch({ text: event.currentTarget.value })}
              className="w-full resize-none rounded-xl border border-border bg-elevated px-3 py-2.5 text-sm text-text outline-none transition-colors duration-150 placeholder:text-text-faint focus:border-accent/70"
            />
          </label>

          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Font</span>
              <select
                value={draft.font}
                onChange={(event) =>
                  patch({ font: event.currentTarget.value as MessageFont })
                }
                className={SELECT_CLASS}
              >
                {MESSAGE_FONTS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>

            <label className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Animation</span>
              <select
                value={draft.animation}
                onChange={(event) =>
                  patch({ animation: event.currentTarget.value as MessageAnimation })
                }
                className={SELECT_CLASS}
              >
                {MESSAGE_ANIMATIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Position</span>
              <select
                value={draft.position}
                onChange={(event) =>
                  patch({ position: event.currentTarget.value as MessagePosition })
                }
                className={SELECT_CLASS}
              >
                {POSITIONS.map((position) => (
                  <option key={position} value={position}>
                    {position}
                  </option>
                ))}
              </select>
            </label>

            <label className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Sound notification</span>
              <select
                value={draft.sound}
                onChange={(event) =>
                  patch({ sound: event.currentTarget.value as MessageSound })
                }
                className={SELECT_CLASS}
              >
                {SOUND_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="flex items-end gap-3">
            <NumberField
              label="Font size"
              value={draft.fontSize}
              max={400}
              suffix="px"
              onChange={(fontSize) => patch({ fontSize })}
            />
            <NumberField
              label="Display duration"
              value={draft.durationSeconds}
              max={600}
              suffix="s"
              onChange={(durationSeconds) => patch({ durationSeconds })}
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Text colour</span>
              <input
                type="color"
                value={draft.color}
                onChange={(event) => patch({ color: event.currentTarget.value })}
                className="h-11 w-full cursor-pointer rounded-xl border border-border bg-elevated p-1"
              />
            </label>

            <label className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Background colour</span>
              <div className="flex items-center gap-2">
                <input
                  type="color"
                  disabled={!draft.backgroundEnabled}
                  value={draft.backgroundColor}
                  onChange={(event) => patch({ backgroundColor: event.currentTarget.value })}
                  className="h-11 w-full cursor-pointer rounded-xl border border-border bg-elevated p-1 disabled:pointer-events-none disabled:opacity-40"
                />
                <button
                  type="button"
                  role="switch"
                  aria-checked={draft.backgroundEnabled}
                  aria-label="Enable message background"
                  onClick={() => patch({ backgroundEnabled: !draft.backgroundEnabled })}
                  className={cn(
                    "relative h-6 w-11 shrink-0 rounded-full border transition-colors duration-200",
                    draft.backgroundEnabled
                      ? "border-accent/60 bg-accent/80"
                      : "border-border bg-elevated",
                  )}
                >
                  <span
                    className={cn(
                      "absolute top-0.5 rounded-full bg-white transition-all duration-200",
                      draft.backgroundEnabled ? "left-[22px]" : "left-0.5",
                    )}
                    style={{ height: 18, width: 18 }}
                  />
                </button>
              </div>
            </label>
          </div>

          {error ? (
            <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
              {error}
            </p>
          ) : null}
        </div>

        {/* ------------------------------------------------- preview column */}
        <div className="flex min-w-0 flex-col gap-3">
          <div className="flex items-center justify-between gap-3">
            <span className={LABEL_CLASS}>Live preview</span>
            <div className="flex items-center gap-1">
              {draft.sound !== "none" ? (
                <IconButton
                  label="Play the selected sound"
                  size="sm"
                  onClick={() => playSound(draft.sound)}
                >
                  <Play size={15} />
                </IconButton>
              ) : null}
              <Button
                size="sm"
                icon={<RotateCcw size={14} />}
                onClick={() => setReplay((value) => value + 1)}
              >
                Replay animation
              </Button>
            </div>
          </div>

          <div
            className={cn(
              "flex min-h-[320px] flex-1 rounded-2xl border border-hairline bg-display px-6 py-6",
              POSITION_ALIGN[draft.position],
            )}
          >
            <MessageSurface
              message={draft.text.trim().length === 0 ? null : preview}
              replayKey={replay}
              style={{ fontSize: previewScale }}
            />
          </div>

          <p className="text-[12px] text-text-faint">
            The preview reproduces the display window&apos;s font, colours and entrance
            animation, and is shown for {draft.durationSeconds}s on the display.
          </p>
        </div>
      </div>
    </Modal>
  );
}
