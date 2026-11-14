import type { Variants } from "framer-motion";
import type { MessageAnimation, MessageFont } from "../types";

/**
 * Presentation typography, motion and sound vocabulary for messages.
 *
 * The preview surface inside the control window and the real presentation
 * surface in the display window render a message through the *same* helpers, so
 * what the operator configures is exactly what the audience sees.
 */

export interface MessageFontOption {
  value: MessageFont;
  label: string;
}

export interface MessageAnimationOption {
  value: MessageAnimation;
  label: string;
}

export const MESSAGE_FONTS: MessageFontOption[] = [
  { value: "sans", label: "Sans (Inter)" },
  { value: "mono", label: "Mono (JetBrains)" },
  { value: "serif", label: "Serif" },
  { value: "display", label: "Display" },
];

export const MESSAGE_ANIMATIONS: MessageAnimationOption[] = [
  { value: "none", label: "None" },
  { value: "fade", label: "Fade" },
  { value: "slide-up", label: "Slide up" },
  { value: "zoom", label: "Zoom" },
  { value: "pulse", label: "Pulse" },
];

/** `PresentationOptions.timerFormat` is free-form; only these two are real. */
const FONT_FAMILY: Record<string, string> = {
  sans: "var(--font-sans)",
  mono: "var(--font-mono)",
  serif: "ui-serif, Georgia, Cambria, 'Times New Roman', serif",
  display: "'Inter', ui-sans-serif, system-ui, sans-serif",
};

/** CSS `font-family` for a message's stored font, defaulting to the UI face. */
export function messageFontFamily(font: string | null | undefined): string {
  return FONT_FAMILY[font ?? "sans"] ?? FONT_FAMILY.sans;
}

/**
 * Framer Motion variants for a message appearance and disappearance.
 *
 * `"none"` still returns a variant so the call site can animate unconditionally;
 * it simply resolves to the resting state, and leaves instantly. The `pulse`
 * variant also loops while the message is on screen. The `exit` state is what
 * `AnimatePresence` plays when a message is replaced or its display duration
 * elapses, so consecutive messages cross-fade instead of flickering.
 */
export function messageAnimationVariants(
  animation: string | null | undefined,
): Variants {
  switch (animation) {
    case "fade":
      return {
        hidden: { opacity: 0 },
        visible: { opacity: 1, transition: { duration: 0.5, ease: "easeOut" } },
        exit: { opacity: 0, transition: { duration: 0.32, ease: "easeIn" } },
      };
    case "slide-up":
      return {
        hidden: { opacity: 0, y: 48 },
        visible: { opacity: 1, y: 0, transition: { duration: 0.5, ease: [0.22, 1, 0.36, 1] } },
        exit: { opacity: 0, y: -32, transition: { duration: 0.32, ease: "easeIn" } },
      };
    case "zoom":
      return {
        hidden: { opacity: 0, scale: 0.82 },
        visible: { opacity: 1, scale: 1, transition: { duration: 0.45, ease: [0.22, 1, 0.36, 1] } },
        exit: { opacity: 0, scale: 0.9, transition: { duration: 0.28, ease: "easeIn" } },
      };
    case "pulse":
      return {
        hidden: { opacity: 0, scale: 0.96 },
        visible: {
          opacity: 1,
          scale: [1, 1.035, 1],
          transition: {
            duration: 1.8,
            ease: "easeInOut",
            repeat: Infinity,
            repeatType: "loop",
            opacity: { duration: 0.4 },
          },
        },
        exit: { opacity: 0, scale: 0.96, transition: { duration: 0.28, ease: "easeIn" } },
      };
    default:
      return {
        hidden: { opacity: 1 },
        visible: { opacity: 1 },
        exit: { opacity: 0, transition: { duration: 0 } },
      };
  }
}

/** Same variants, restricted to the entrance so a preview can replay it. */
export function isLoopingAnimation(animation: string | null | undefined): boolean {
  return animation === "pulse";
}
