export type MessagePosition = "top" | "center" | "bottom";

/** Typography family a message is rendered in. */
export type MessageFont = "sans" | "mono" | "serif" | "display";

/** Entrance motion played when a message appears on the display window. */
export type MessageAnimation = "none" | "fade" | "slide-up" | "zoom" | "pulse";

/** Identifier of a built-in notification sound, or `"none"` for silence. */
export type MessageSound = "none" | "chime" | "bell" | "beep" | "alarm";

export interface MessagePreset {
  id: string;
  name: string;
  text: string;
  /** Presentation font size in pixels, tuned for large-format display. */
  fontSize: number;
  color: string;
  backgroundColor: string | null;
  position: MessagePosition;
  font: MessageFont;
  animation: MessageAnimation;
  durationMs: number;
  /** Sound played when this message appears, if any. */
  sound: MessageSound;
  createdAt: number;
}
