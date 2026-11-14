/** Normalised monitor description shared by the Rust and TypeScript layers. */
export interface MonitorInfo {
  /**
   * Stable identity: the monitor name plus its virtual-desktop origin. The
   * index Windows reports a monitor under is not stable across reconnects, so
   * selection and persistence key off this value instead.
   */
  key: string;
  name: string | null;
  width: number;
  height: number;
  x: number;
  y: number;
  scaleFactor: number;
  isPrimary: boolean;
  /** True for the monitor that currently hosts the control window. */
  isCurrent: boolean;
}

/** Lifecycle state of the native presentation window. */
export interface DisplayWindowState {
  open: boolean;
  fullscreen: boolean;
  maximized: boolean;
  /** Whether the window is pinned above other windows. */
  alwaysOnTop: boolean;
  /**
   * True while presentation mode is active. Presentation mode is entered
   * through the control window and combines fullscreen, always-on-top and a
   * chrome-free frame; leaving it restores the previous window mode.
   */
  presentation: boolean;
  monitorKey: string | null;
  monitorName: string | null;
}

export const CLOSED_DISPLAY_WINDOW: DisplayWindowState = {
  open: false,
  fullscreen: false,
  maximized: false,
  alwaysOnTop: false,
  presentation: false,
  monitorKey: null,
  monitorName: null,
};

/** Physical placement of the presentation window, used to remember it. */
export interface DisplayGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
  monitorKey: string | null;
}

// ---------------------------------------------------------------------------
// Display customisation
// ---------------------------------------------------------------------------

/** Horizontal alignment of a presentation element. */
export type DisplayAlign = "left" | "center" | "right";

/** Vertical band a presentation element is placed in. */
export type DisplayBand = "top" | "center" | "bottom";

/** Corner the overtime counter is anchored to. */
export type DisplayCorner = "top-left" | "top-right" | "bottom-left" | "bottom-right";

/**
 * Typography for one presentation text element.
 *
 * `scale` multiplies the element's responsive base size rather than replacing
 * it, so a configuration that reads well on an external 4K panel still fits a
 * laptop screen.
 */
export interface DisplayTextStyle {
  /** Font family key: `sans`, `mono`, `serif` or `display`. */
  font: string;
  /** Multiplier applied to the element's responsive base size. */
  scale: number;
  /** CSS font weight. */
  weight: number;
  align: DisplayAlign;
}

/** Where each presentation element sits and what is shown at all. */
export interface DisplayLayoutConfig {
  timerPosition: DisplayBand;
  messagePosition: DisplayBand;
  progressPosition: DisplayBand;
  overtimeCorner: DisplayCorner;
  /** Show the running session's name. */
  showSessionInfo: boolean;
  /** Show `Stage n / m · name`. */
  showStageInfo: boolean;
  /** Show the timer or session name above the digits. */
  showTimerName: boolean;
  /** Show the elapsed-progress indicator. */
  showProgressBar: boolean;
  /** Allow messages to be presented. */
  showMessages: boolean;
  /** Vertical rhythm multiplier applied to the gaps between elements. */
  spacing: number;
  /** Horizontal alignment of the whole presentation. */
  align: DisplayAlign;
}

/** Colours and background of the presentation surface. */
export interface DisplayThemeConfig {
  background: string;
  /** Optional background image, held as a data URL, or `null` for none. */
  backgroundImage: string | null;
  timerColor: string;
  /**
   * Message text colour. An empty string leaves every message in its own
   * colour, as configured in the message library; any value overrides them all.
   */
  messageColor: string;
  progressColor: string;
  overtimeColor: string;
}

/**
 * The complete customisation of the presentation surface.
 *
 * Rust stores and broadcasts this untouched — it is styling, not state — but
 * the shape is mirrored in `src-tauri/src/display/mod.rs` so both sides share
 * one contract, exactly as `DisplayMessage` and `SessionPresentation` do.
 */
export interface DisplayStyle {
  layout: DisplayLayoutConfig;
  timer: DisplayTextStyle;
  messageText: DisplayTextStyle;
  theme: DisplayThemeConfig;
}

/** A reusable presentation layout: a name plus a complete style. */
export interface DisplayLayoutPreset {
  id: string;
  name: string;
  /** Built-in layouts ship with the application and cannot be deleted. */
  builtIn: boolean;
  style: DisplayStyle;
}

/** A message preset prepared for presentation on the display window. */
export interface DisplayMessage {
  id: string;
  name: string;
  text: string;
  fontSize: number;
  color: string;
  backgroundColor: string | null;
  position: string;
  font: string;
  animation: string;
  /** How long the message stays on screen, in milliseconds; `0` is sticky. */
  durationMs: number;
}

/**
 * The stage a running session is currently on, pushed by the Rust session
 * engine. Mirrors `SessionPresentation` in `src-tauri/src/display/mod.rs`; the
 * display window renders it directly so no control-window round trip is needed.
 */
export interface SessionPresentation {
  sessionId: string;
  sessionName: string;
  stageIndex: number;
  stageCount: number;
  stageName: string;
  awaitingManual: boolean;
  transitionCountdownMs: number | null;
  message: DisplayMessage | null;
}

/**
 * Everything the display window needs to render itself.
 *
 * Rust owns this state: the control window pushes it and the display window
 * reads it back, which is the same command + broadcast contract the timer
 * engine adopts in Milestone 3.
 */
export interface PresentationOptions {
  timerFormat: string;
  message: DisplayMessage | null;
  style: DisplayStyle;
}

/** Arguments accepted by the `open_display_window` command. */
export interface OpenDisplayOptions {
  monitorKey?: string | null;
  x?: number | null;
  y?: number | null;
  width?: number;
  height?: number;
  fullscreen?: boolean;
  alwaysOnTop?: boolean;
}
