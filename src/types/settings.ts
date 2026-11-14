import type { TimerFormat } from "../lib/time";
import type { MessageSound } from "./message";

export type StartupBehaviour = "dashboard" | "restore";

export type InterfaceDensity = "comfortable" | "compact";

export interface GeneralSettings {
  startupBehaviour: StartupBehaviour;
  defaultTimerFormat: TimerFormat;
  confirmBeforeStopping: boolean;
  timerAdjustmentMinutes: number;
}

export interface AppearanceSettings {
  accentColor: string;
  density: InterfaceDensity;
}

/**
 * Window-level presentation preferences.
 *
 * The *styling* of the presentation surface (layout, typography, colours) lives
 * in `displayConfigStore`, which owns one style document; this section only
 * holds how the native window itself opens and behaves.
 */
export interface DisplaySettings {
  /** Stable monitor key (`name@x,y`) the presentation window prefers. */
  defaultMonitorKey: string | null;
  defaultWidth: number;
  defaultHeight: number;
  startFullscreen: boolean;
  /** Pin the presentation window above other windows. */
  alwaysOnTop: boolean;
}

export interface NotificationSettings {
  /** Master switch for every audio cue. */
  soundsEnabled: boolean;
  /** Sound played when an ordinary (non-session) countdown completes. */
  completionSound: MessageSound;
  /** Play the cue at warning thresholds. */
  warningSounds: boolean;
  /** Play a cue when a stage starts. */
  stageSounds: boolean;
  /** Play a cue when the whole session finishes. */
  sessionSounds: boolean;
}

export interface AppSettings {
  general: GeneralSettings;
  appearance: AppearanceSettings;
  display: DisplaySettings;
  notifications: NotificationSettings;
}
