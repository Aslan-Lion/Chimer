import type { AppSettings, NotificationSettings } from "../types";
import { createStore } from "./createStore";
import { readStorage, writeStorage } from "../lib/storage";
import { SOUND_OPTIONS } from "../lib/sound";

const STORAGE_KEY = "app-timer.settings";

export const DEFAULT_SETTINGS: AppSettings = {
  general: {
    startupBehaviour: "dashboard",
    defaultTimerFormat: "hh:mm:ss",
    confirmBeforeStopping: true,
    timerAdjustmentMinutes: 1,
  },
  appearance: {
    accentColor: "#8B9DFF",
    density: "comfortable",
  },
  display: {
    defaultMonitorKey: null,
    defaultWidth: 1280,
    defaultHeight: 720,
    startFullscreen: false,
    alwaysOnTop: false,
  },
  notifications: {
    soundsEnabled: true,
    completionSound: "chime",
    warningSounds: true,
    stageSounds: true,
    sessionSounds: true,
  },
};

const SOUND_VALUES = new Set(SOUND_OPTIONS.map((option) => option.value));

/**
 * Rebuilds a stored settings document field by field.
 *
 * Settings written by an earlier milestone still load: fields that moved into
 * the display style are dropped rather than carried through, and a stored sound
 * that is no longer offered falls back to the default instead of reaching the
 * audio engine as an unknown value.
 */
function mergeSettings(stored: Partial<AppSettings>): AppSettings {
  const notifications: Partial<NotificationSettings> = stored.notifications ?? {};
  const completionSound =
    notifications.completionSound && SOUND_VALUES.has(notifications.completionSound)
      ? notifications.completionSound
      : DEFAULT_SETTINGS.notifications.completionSound;

  const adjustmentMinutes = stored.general?.timerAdjustmentMinutes;
  const allowedAdjustmentMinutes = [1, 2, 5, 10, 15, 30, 60];

  return {
    general: {
      ...DEFAULT_SETTINGS.general,
      ...stored.general,
      timerAdjustmentMinutes:
        typeof adjustmentMinutes === "number" &&
        Number.isFinite(adjustmentMinutes) &&
        allowedAdjustmentMinutes.includes(adjustmentMinutes)
          ? adjustmentMinutes
          : DEFAULT_SETTINGS.general.timerAdjustmentMinutes,
    },
    appearance: { ...DEFAULT_SETTINGS.appearance, ...stored.appearance },
    display: {
      defaultMonitorKey:
        stored.display?.defaultMonitorKey ?? DEFAULT_SETTINGS.display.defaultMonitorKey,
      defaultWidth: stored.display?.defaultWidth ?? DEFAULT_SETTINGS.display.defaultWidth,
      defaultHeight: stored.display?.defaultHeight ?? DEFAULT_SETTINGS.display.defaultHeight,
      startFullscreen:
        stored.display?.startFullscreen ?? DEFAULT_SETTINGS.display.startFullscreen,
      alwaysOnTop: stored.display?.alwaysOnTop ?? DEFAULT_SETTINGS.display.alwaysOnTop,
    },
    notifications: {
      soundsEnabled:
        notifications.soundsEnabled ?? DEFAULT_SETTINGS.notifications.soundsEnabled,
      completionSound,
      warningSounds: notifications.warningSounds ?? DEFAULT_SETTINGS.notifications.warningSounds,
      stageSounds: notifications.stageSounds ?? DEFAULT_SETTINGS.notifications.stageSounds,
      sessionSounds: notifications.sessionSounds ?? DEFAULT_SETTINGS.notifications.sessionSounds,
    },
  };
}

interface SettingsState {
  settings: AppSettings;
}

export const settingsStore = createStore<SettingsState>({
  settings: mergeSettings(readStorage<Partial<AppSettings>>(STORAGE_KEY, {})),
});

settingsStore.subscribe(() => {
  writeStorage(STORAGE_KEY, settingsStore.get().settings);
});

/** Patch a single settings section; unused sections keep their values. */
export function updateSettings<K extends keyof AppSettings>(
  section: K,
  patch: Partial<AppSettings[K]>,
): void {
  settingsStore.set((previous) => ({
    settings: {
      ...previous.settings,
      [section]: { ...previous.settings[section], ...patch },
    },
  }));
}

export function resetSettings(): void {
  settingsStore.set({ settings: DEFAULT_SETTINGS });
}
