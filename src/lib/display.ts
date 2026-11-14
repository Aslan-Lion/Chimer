import type {
  DisplayAlign,
  DisplayBand,
  DisplayCorner,
  DisplayLayoutConfig,
  DisplayLayoutPreset,
  DisplayStyle,
  DisplayTextStyle,
  DisplayThemeConfig,
  PresentationOptions,
} from "../types";
import { clamp } from "./time";

/**
 * Presentation vocabulary: layout, typography and colour.
 *
 * The real presentation surface and the live preview inside the control window
 * both render through these values, so what the operator configures is exactly
 * what the audience sees. Nothing here holds timer state — the style is
 * styling, mirrored verbatim in `src-tauri/src/display/mod.rs`.
 */

export const DISPLAY_BANDS: Array<{ value: DisplayBand; label: string }> = [
  { value: "top", label: "Top" },
  { value: "center", label: "Centre" },
  { value: "bottom", label: "Bottom" },
];

export const DISPLAY_ALIGNS: Array<{ value: DisplayAlign; label: string }> = [
  { value: "left", label: "Left" },
  { value: "center", label: "Centre" },
  { value: "right", label: "Right" },
];

export const DISPLAY_CORNERS: Array<{ value: DisplayCorner; label: string }> = [
  { value: "top-left", label: "Top left" },
  { value: "top-right", label: "Top right" },
  { value: "bottom-left", label: "Bottom left" },
  { value: "bottom-right", label: "Bottom right" },
];

/** CSS font weights offered for the timer digits and for messages. */
export const DISPLAY_WEIGHTS = [300, 400, 500, 600, 700, 800];

/**
 * Sentinel for `DisplayThemeConfig.messageColor`.
 *
 * An empty string means "leave every message in its own colour" — the message
 * library then decides. Any colour value overrides them all.
 */
export const MESSAGE_COLOR_FROM_MESSAGE = "";

export const DEFAULT_DISPLAY_STYLE: DisplayStyle = {
  layout: {
    timerPosition: "center",
    messagePosition: "bottom",
    progressPosition: "bottom",
    overtimeCorner: "bottom-right",
    showSessionInfo: false,
    showStageInfo: true,
    showTimerName: true,
    showProgressBar: true,
    showMessages: true,
    spacing: 1,
    align: "center",
  },
  // The digits are a monospaced tabular readout; messages are prose.
  timer: { font: "mono", scale: 1, weight: 600, align: "center" },
  messageText: { font: "sans", scale: 1, weight: 600, align: "center" },
  theme: {
    background: "#050505",
    backgroundImage: null,
    timerColor: "#FFFFFF",
    messageColor: MESSAGE_COLOR_FROM_MESSAGE,
    progressColor: "#8B9DFF",
    overtimeColor: "#FF4545",
  },
};

/** A partial style, used to layer a change over an existing document. */
export interface DisplayStylePatch {
  layout?: Partial<DisplayLayoutConfig>;
  timer?: Partial<DisplayTextStyle>;
  messageText?: Partial<DisplayTextStyle>;
  theme?: Partial<DisplayThemeConfig>;
}

/** Layers a patch over a complete style, section by section. */
export function mergeStyle(base: DisplayStyle, patch: DisplayStylePatch): DisplayStyle {
  return {
    layout: { ...base.layout, ...patch.layout },
    timer: { ...base.timer, ...patch.timer },
    messageText: { ...base.messageText, ...patch.messageText },
    theme: { ...base.theme, ...patch.theme },
  };
}

/**
 * Fills in anything a stored document predates, so an older save keeps loading
 * instead of leaving `undefined` values in the interface.
 */
export function normaliseStyle(style: Partial<DisplayStyle> | null | undefined): DisplayStyle {
  return mergeStyle(DEFAULT_DISPLAY_STYLE, {
    layout: style?.layout,
    timer: style?.timer,
    messageText: style?.messageText,
    theme: style?.theme,
  });
}

/** Deep equality of two styles, used to flag a layout as modified. */
export function stylesEqual(a: DisplayStyle, b: DisplayStyle): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * Ships with the application. A built-in is never written to — editing the
 * applied style leaves its definition untouched — and it can never be deleted.
 */
export const BUILT_IN_LAYOUTS: DisplayLayoutPreset[] = [
  {
    id: "layout-presentation",
    name: "Presentation",
    builtIn: true,
    style: DEFAULT_DISPLAY_STYLE,
  },
  {
    id: "layout-minimal",
    name: "Minimal",
    builtIn: true,
    style: mergeStyle(DEFAULT_DISPLAY_STYLE, {
      layout: {
        showTimerName: false,
        showSessionInfo: false,
        showStageInfo: false,
        showProgressBar: false,
        showMessages: false,
        spacing: 1.1,
      },
      timer: { scale: 1.18, weight: 700 },
    }),
  },
  {
    id: "layout-session",
    name: "Session",
    builtIn: true,
    style: mergeStyle(DEFAULT_DISPLAY_STYLE, {
      layout: {
        timerPosition: "center",
        messagePosition: "bottom",
        showSessionInfo: true,
        showStageInfo: true,
        showProgressBar: true,
        showMessages: true,
      },
      timer: { scale: 0.9 },
    }),
  },
  {
    id: "layout-focus",
    name: "Focus",
    builtIn: true,
    style: mergeStyle(DEFAULT_DISPLAY_STYLE, {
      layout: {
        showTimerName: false,
        showSessionInfo: false,
        showStageInfo: false,
        showProgressBar: false,
        showMessages: false,
        spacing: 1.3,
      },
      timer: { scale: 1.3, weight: 700 },
      theme: { background: "#000000" },
    }),
  },
];

export const DEFAULT_LAYOUT_ID = BUILT_IN_LAYOUTS[0].id;

/** Finds a layout by id, searching both built-in and custom definitions. */
export function findLayout(
  layouts: DisplayLayoutPreset[],
  id: string | null,
): DisplayLayoutPreset | null {
  if (id === null) return null;
  return layouts.find((layout) => layout.id === id) ?? null;
}

/** Repairs stored custom layouts; a custom layout is never marked built-in. */
export function normaliseCustomLayouts(value: unknown): DisplayLayoutPreset[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter((entry): entry is DisplayLayoutPreset => {
      const candidate = entry as Partial<DisplayLayoutPreset> | null;
      return Boolean(candidate && typeof candidate.id === "string" && typeof candidate.name === "string");
    })
    .map((entry) => ({
      id: entry.id,
      name: entry.name,
      builtIn: false,
      style: normaliseStyle(entry.style),
    }));
}

/** The fallback presentation document, used before Rust has pushed one. */
export const DEFAULT_PRESENTATION_OPTIONS: PresentationOptions = {
  timerFormat: "hh:mm:ss",
  message: null,
  style: DEFAULT_DISPLAY_STYLE,
};

// ---------------------------------------------------------------------------
// Responsive sizing
// ---------------------------------------------------------------------------
//
// Every size is expressed in container query units (`cqw`), not viewport units.
// `DisplaySurface` declares itself an inline-size container, so the same numbers
// resolve against the presentation window on one screen and against the live
// preview card in the control window on another — one style, two scales.

/** A configured scale is kept inside a range that still fits a small window. */
function safeScale(scale: number): number {
  if (!Number.isFinite(scale)) return 1;
  return clamp(scale, 0.4, 2.2);
}

/**
 * Font size of the countdown digits.
 *
 * The element keeps its responsive base and the operator's scale multiplies it,
 * so a configuration tuned on a 4K panel still fits a laptop screen.
 */
export function timerFontSize(scale: number): string {
  return `calc(clamp(88px, 16cqw, 320px) * ${safeScale(scale)})`;
}

/**
 * Font size of the `TIME IS UP` completion notice.
 *
 * It carries eleven glyphs rather than eight digits, so it uses a smaller base
 * and the same scale — a configuration that fits the digits fits the notice.
 */
export function timeUpFontSize(scale: number): string {
  return `calc(clamp(48px, 9.5cqw, 220px) * ${safeScale(scale)})`;
}

/**
 * Font size of a message.
 *
 * A message carries its own size from the library; the display style scales it,
 * which is what makes one typography setting apply to every message without
 * destroying the per-message tuning.
 */
export function messageFontSize(fontSize: number, scale: number): string {
  const size = Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 104;
  const share = Math.min(size, 120) / 16;
  return `calc(clamp(18px, ${share}cqw, 120px) * ${safeScale(scale)})`;
}

/** Vertical rhythm between presentation elements. */
export function spacingGap(spacing: number): string {
  const value = Number.isFinite(spacing) && spacing > 0 ? clamp(spacing, 0.4, 2.4) : 1;
  return `calc(clamp(18px, 2.4cqw, 44px) * ${value})`;
}

/** Font size of the overtime counter and of the transition banner. */
export function auxiliaryFontSize(): string {
  return "clamp(15px, 1.8cqw, 38px)";
}

/** Horizontal insets of the presentation surface. */
export function surfacePadding(): string {
  return "clamp(20px, 6cqw, 120px)";
}
