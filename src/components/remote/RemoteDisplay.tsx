import { Wifi, WifiOff } from "lucide-react";
import { remoteStore } from "../../stores/remoteStore";
import { DisplaySurface, EMPTY_SCENE, sceneFromPresentation } from "../display/DisplaySurface";
import type { TimerFormat } from "../../lib/time";

/** `PresentationOptions.timerFormat` is a free-form string; only these two are real. */
function presentFormat(value: string): TimerFormat {
  return value === "mm:ss" ? "mm:ss" : "hh:mm:ss";
}

/**
 * The remote display: a read-only mirror of the presentation surface.
 *
 * It reuses the exact `DisplaySurface` the native display window renders, fed by
 * the same `SessionPresentation` shape the engine pushes, so a phone propped up
 * as a confidence monitor shows what the desktop shows. It issues no commands —
 * there is no control here at all — and it owns no clock: the digits, the
 * progress and the overtime counter are the host's values, which is why a
 * display that connects mid-countdown draws the right time immediately.
 */
export function RemoteDisplay() {
  const { timer, sessionPresentation, presentation, connected } = remoteStore.use();

  const scene =
    timer === null && sessionPresentation === null
      ? EMPTY_SCENE
      : sceneFromPresentation(timer, sessionPresentation, presentation.message);

  return (
    /*
     * `fixed inset-0` rather than `100vw`/`100dvh`: a phone browser's address
     * bar changes the dynamic viewport, and `100vw` counts the scrollbar
     * gutter. Pinning to the viewport keeps the presentation exactly full-bleed
     * in portrait and landscape, and the surface itself scales from its own
     * container (it declares an inline-size container), so nothing needs to
     * know the device's pixel size.
     */
    <div className="fixed inset-0 overflow-hidden bg-[#050505]">
      <DisplaySurface
        style={presentation.style}
        scene={scene}
        timerFormat={presentFormat(presentation.timerFormat)}
      />

      {/* Quiet status chips, anchored to the corners so they never cover the
          digits in the middle, and kept clear of a notch or rounded corner. */}
      {!connected ? (
        <div className="pointer-events-none absolute top-[max(0.75rem,env(safe-area-inset-top))] left-1/2 z-10 flex max-w-[80vw] -translate-x-1/2 items-center gap-1.5 rounded-full border border-[#4a4120] bg-[#241f10] px-3 py-1 text-[11px] font-medium text-warning">
          <WifiOff size={12} className="shrink-0" />
          <span className="truncate">Reconnecting to the host…</span>
        </div>
      ) : (
        <div
          aria-hidden
          className="pointer-events-none absolute top-[max(0.75rem,env(safe-area-inset-top))] right-[max(0.75rem,env(safe-area-inset-right))] z-10 flex items-center gap-1.5 rounded-full border border-white/10 bg-black/40 px-2.5 py-1 text-[10px] text-white/50"
        >
          <Wifi size={11} />
          live
        </div>
      )}
    </div>
  );
}
