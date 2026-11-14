import type { CSSProperties } from "react";
import { motion } from "framer-motion";
import { cn } from "../../lib/cn";

/**
 * The completion warning colour, fixed by the timer specification rather than
 * taken from the theme palette. The display style may override it.
 */
const TIME_UP_COLOR = "#ff4545";

export interface TimeIsUpProps {
  /** Type scale for the call site; the pulse itself is identical everywhere. */
  className?: string;
  /** Optional colour override supplied by the presentation style. */
  style?: CSSProperties;
}

/**
 * Completion presentation: replaces the numerical countdown entirely.
 *
 * It is driven purely by the engine's `completed` status, so it appears the
 * moment the Rust completion timestamp is reached and disappears the moment the
 * engine leaves that state — there is no frontend timer behind it. The pulse is
 * a slow, shallow opacity and scale loop: readable at presentation distance
 * without competing with the overtime counter.
 */
export function TimeIsUp({ className, style }: TimeIsUpProps) {
  return (
    <motion.span
      role="status"
      aria-label="Time is up"
      className={cn(
        "font-mono tabnum leading-none font-semibold tracking-[0.06em]",
        className,
      )}
      style={{ color: TIME_UP_COLOR, ...style }}
      animate={{ opacity: [1, 0.55, 1], scale: [1, 0.985, 1] }}
      transition={{ duration: 2.2, repeat: Infinity, ease: "easeInOut" }}
    >
      TIME IS UP
    </motion.span>
  );
}
