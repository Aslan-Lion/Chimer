import { useMemo } from "react";
import type { DisplayStyle } from "../../types";
import { sessionStore } from "../../stores/sessionStore";
import { timerStore } from "../../stores/timerStore";
import { DisplaySurface, sceneFromSnapshot } from "./DisplaySurface";
import type { DisplayScene, MessageSource } from "./DisplaySurface";

/**
 * Stand-in content for the preview.
 *
 * The preview exists so the operator can tune a layout without a countdown
 * running, so when nothing is live it shows a representative scene rather than
 * an empty box. It is confined to the control window and clearly labelled: the
 * real presentation window only ever renders engine snapshots.
 */
const DEMO_MESSAGE: MessageSource = {
  text: "10 minutes remaining",
  color: "#F4F4F5",
  backgroundColor: null,
  fontSize: 104,
  font: "sans",
  animation: "fade",
  durationMs: 0,
};

function demoScene(message: MessageSource | null): DisplayScene {
  return {
    timerName: "Examination Block",
    armed: true,
    completed: false,
    remainingMs: 12 * 60 * 1000 + 34 * 1000,
    progress: 0.42,
    overtimeMs: 0,
    activeSession: true,
    sessionName: "Examination Block",
    stageIndex: 1,
    stageCount: 3,
    stageName: "Written paper",
    awaitingManual: false,
    transitionCountdownMs: null,
    message: message ?? DEMO_MESSAGE,
    messageKey: message ? "preview:selected" : "preview:demo",
    sessionComplete: false,
  };
}

export interface DisplayPreviewProps {
  style: DisplayStyle;
  /** Message chosen for the preview, or `null` to fall back to a sample. */
  message: MessageSource | null;
  /** Bumping this replays the animation so an exit and entrance can be seen. */
  nonce: number;
}

/**
 * Live preview of the presentation surface.
 *
 * It renders through the same `DisplaySurface` the display window uses, so the
 * layout, typography, colours and message motion shown here are exactly what
 * the audience sees. When a countdown or a session is already running the
 * preview follows it live; otherwise it shows a representative scene. Either
 * way it renders the *applied* style, so an edit appears immediately without
 * reopening the display window.
 */
export function DisplayPreview({ style, message, nonce }: DisplayPreviewProps) {
  const { snapshot } = timerStore.use();
  const { snapshot: session } = sessionStore.use();

  const live =
    (snapshot !== null && snapshot.timerId !== null) ||
    (session !== null && session.sessionId !== null);

  const scene = useMemo(
    () => (live ? sceneFromSnapshot(snapshot, session, message) : demoScene(message)),
    [live, snapshot, session, message],
  );

  return (
    <div className="overflow-hidden rounded-xl border border-hairline">
      <div className="aspect-video w-full">
        <DisplaySurface style={style} scene={scene} timerFormat="hh:mm:ss" replayNonce={nonce} />
      </div>
    </div>
  );
}
