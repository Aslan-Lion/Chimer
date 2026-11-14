import type { CSSProperties } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { cn } from "../../lib/cn";
import { messageAnimationVariants, messageFontFamily } from "../../lib/message";

/**
 * The smallest shape a message has to have to be rendered. `MessagePreset`,
 * `StageMessage` and `DisplayMessage` all satisfy it, so the editor's live
 * preview and the real presentation surface share one renderer.
 */
export interface MessageSource {
  text: string;
  color: string;
  backgroundColor: string | null;
  /** Presentation size in pixels, used by the display style to scale the text. */
  fontSize?: number | null;
  font?: string | null;
  animation?: string | null;
  /** How long the message stays on screen, in milliseconds; `0` is sticky. */
  durationMs?: number | null;
}

export interface MessageSurfaceProps {
  message: MessageSource | null;
  /** Shown when there is no message yet, so the preview pane is never blank. */
  placeholder?: string;
  /** Type scale for the call site; the surface itself never sets a size. */
  className?: string;
  style?: CSSProperties;
  /** Changing this value replays the entrance animation. */
  replayKey?: string | number;
}

/**
 * Renders a message exactly as the display window will: same font, colour,
 * background, entrance motion and exit motion.
 *
 * `AnimatePresence` owns the lifetime of the element, so a message that is
 * replaced — or whose display duration has elapsed — leaves through its own
 * exit transition instead of vanishing. The animation is pure presentation: it
 * never touches the countdown, which stays the Rust engine's.
 *
 * Passing neither a message nor a placeholder renders nothing, which is how the
 * presentation surface clears a message while still playing its exit.
 */
export function MessageSurface({
  message,
  placeholder = "Message preview",
  className,
  style,
  replayKey,
}: MessageSurfaceProps) {
  const variants = messageAnimationVariants(message?.animation);
  const hasBackground = Boolean(message?.backgroundColor);
  const text = message?.text || placeholder;

  // A message and the placeholder are different elements, so swapping between
  // them still plays an exit and an entrance rather than snapping.
  const key = message ? (replayKey ?? message.text ?? "message") : "placeholder";

  return (
    <AnimatePresence mode="wait">
      {text ? (
        <motion.div
          key={key}
          variants={variants}
          initial="hidden"
          animate="visible"
          exit="exit"
          className={cn(
            "inline-flex max-w-full items-center justify-center text-center leading-tight font-semibold",
            hasBackground && "rounded-2xl px-[0.6em] py-[0.35em]",
            !message && "text-text-faint",
            className,
          )}
          style={{
            color: message?.color ?? undefined,
            backgroundColor: message?.backgroundColor ?? undefined,
            fontFamily: messageFontFamily(message?.font),
            ...style,
          }}
        >
          {text}
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
