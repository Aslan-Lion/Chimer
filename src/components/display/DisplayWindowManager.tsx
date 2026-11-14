import { useEffect, useMemo } from "react";
import type { DisplayMessage, PresentationOptions } from "../../types";
import {
  isDesktopRuntime,
  onDisplayGeometry,
  onDisplayWindowState,
  onMonitorsChanged,
} from "../../services/display";
import { libraryStore } from "../../stores/libraryStore";
import { displayConfigStore } from "../../stores/displayConfigStore";
import { settingsStore } from "../../stores/settingsStore";
import { uiStore } from "../../stores/uiStore";
import {
  applyGeometry,
  applyMonitors,
  applyWindowState,
  refreshDisplay,
  syncPresentationOptions,
} from "../../stores/displayStore";

/**
 * Control-window bridge to the native presentation window.
 *
 * The control window is the only writer of the shared presentation state: it
 * mirrors the native window events into `displayStore` and pushes one
 * `PresentationOptions` document — the applied display style plus the message
 * currently being presented — down to Rust, which broadcasts it to the display
 * window. Rust therefore owns the presentation state as it owns the timer, and
 * the display window never holds administrative state of its own.
 *
 * This component renders nothing; it only owns the side effects.
 */
export function DisplayWindowManager() {
  const { settings } = settingsStore.use();
  const { previewMessageId } = uiStore.use();
  const { messages } = libraryStore.use();
  const { style } = displayConfigStore.use();

  // Native window events -> store.
  useEffect(() => {
    if (!isDesktopRuntime()) return;

    let cancelled = false;
    const disposers: Array<() => void> = [];

    void (async () => {
      const registered = await Promise.all([
        onDisplayWindowState(applyWindowState),
        onMonitorsChanged(applyMonitors),
        onDisplayGeometry(applyGeometry),
      ]);

      if (cancelled) registered.forEach((off) => off());
      else disposers.push(...registered);
    })();

    void refreshDisplay();

    return () => {
      cancelled = true;
      disposers.forEach((off) => off());
    };
  }, []);

  // Store -> Rust -> display window. Every field of the display style travels
  // with the message, so changing a colour or a layout updates the open window
  // without touching the countdown.
  const previewMessage = messages.find((message) => message.id === previewMessageId) ?? null;

  const presentation = useMemo<PresentationOptions>(() => {
    const message: DisplayMessage | null = previewMessage
      ? {
          id: previewMessage.id,
          name: previewMessage.name,
          text: previewMessage.text,
          fontSize: previewMessage.fontSize,
          color: previewMessage.color,
          backgroundColor: previewMessage.backgroundColor,
          position: previewMessage.position,
          font: previewMessage.font,
          animation: previewMessage.animation,
          durationMs: previewMessage.durationMs,
        }
      : null;

    return {
      timerFormat: settings.general.defaultTimerFormat,
      message,
      style,
    };
  }, [previewMessage, settings.general.defaultTimerFormat, style]);

  useEffect(() => {
    void syncPresentationOptions(presentation);
  }, [presentation]);

  return null;
}
