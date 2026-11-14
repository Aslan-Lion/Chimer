import { useEffect, useState } from "react";
import type { PresentationOptions, SessionPresentation } from "../../types";
import { DEFAULT_PRESENTATION_OPTIONS } from "../../lib/display";
import {
  getPresentationOptions,
  isDesktopRuntime,
  onPresentationChanged,
} from "../../services/display";
import { getSessionPresentation, onSessionPresentationChanged } from "../../services/session";
import type { TimerFormat } from "../../lib/time";
import { timerStore } from "../../stores/timerStore";
import { DisplaySurface, EMPTY_SCENE, sceneFromPresentation } from "./DisplaySurface";

/** `PresentationOptions.timerFormat` is a free-form string; only these two are real. */
function presentFormat(value: string): TimerFormat {
  return value === "mm:ss" ? "mm:ss" : "hh:mm:ss";
}

/**
 * Presentation surface for the dedicated display window.
 *
 * This is a genuine native window; the component only draws it. It holds no
 * clock and no timer state of its own: the countdown, the progress and the
 * overtime counter come from the snapshot the Rust engine broadcasts, and the
 * layout, typography and colours come from the presentation document Rust
 * re-broadcasts from the control window. Opening this window while a countdown
 * is already running therefore renders the correct state immediately, because
 * both the snapshot and the presentation document are read back from Rust
 * rather than invented here.
 *
 * The stage, the message and the style travel the same route, so the session
 * engine drives the display window without a control-window round trip.
 */
export function DisplayWindow() {
  const [options, setOptions] = useState<PresentationOptions>(DEFAULT_PRESENTATION_OPTIONS);
  const [session, setSession] = useState<SessionPresentation | null>(null);
  const { snapshot } = timerStore.use();

  useEffect(() => {
    if (!isDesktopRuntime()) return;

    let cancelled = false;
    let dispose: (() => void) | null = null;

    void (async () => {
      // Recover the last options in case this window opened after the broadcast.
      const current = await getPresentationOptions();
      if (cancelled) return;
      setOptions(current);

      const unlisten = await onPresentationChanged((next) => setOptions(next));
      if (cancelled) unlisten();
      else dispose = unlisten;
    })();

    return () => {
      cancelled = true;
      dispose?.();
    };
  }, []);

  useEffect(() => {
    if (!isDesktopRuntime()) return;

    let cancelled = false;
    let dispose: (() => void) | null = null;

    void (async () => {
      // The engine owns the stage: recover the last push, then follow the channel.
      const current = await getSessionPresentation();
      if (cancelled) return;
      setSession(current);

      const unlisten = await onSessionPresentationChanged((next) => setSession(next));
      if (cancelled) unlisten();
      else dispose = unlisten;
    })();

    return () => {
      cancelled = true;
      dispose?.();
    };
  }, []);

  const scene = snapshot === null && session === null
    ? EMPTY_SCENE
    : sceneFromPresentation(snapshot, session, options.message);

  return (
    <div className="h-screen w-screen select-none">
      <DisplaySurface style={options.style} scene={scene} timerFormat={presentFormat(options.timerFormat)} />
    </div>
  );
}
