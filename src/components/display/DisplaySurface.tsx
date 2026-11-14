import type { CSSProperties, ReactNode } from "react";
import { useEffect, useState } from "react";
import { formatDuration } from "../../lib/time";
import type { TimerFormat } from "../../lib/time";
import {
  auxiliaryFontSize,
  messageFontSize,
  spacingGap,
  surfacePadding,
  timeUpFontSize,
  timerFontSize,
} from "../../lib/display";
import { messageFontFamily } from "../../lib/message";
import type {
  DisplayAlign,
  DisplayCorner,
  DisplayStyle,
  SessionPresentation,
  SessionSnapshot,
  TimerSnapshot,
} from "../../types";
import { MessageSurface } from "./MessageSurface";
import type { MessageSource } from "./MessageSurface";
import { ProgressBar } from "../timer/ProgressBar";
import { TimeIsUp } from "../timer/TimeIsUp";

export type { MessageSource } from "./MessageSurface";

/**
 * Everything the presentation surface draws, normalised away from the two
 * different shapes the two windows receive.
 *
 * The control window follows `SessionSnapshot`; the display window follows
 * `SessionPresentation`. Both reduce to this scene, so one renderer serves the
 * real window and the live preview and they can never disagree. No field here
 * is computed by the frontend — every value is copied from an engine snapshot.
 */
export interface DisplayScene {
  /** Timer or stage name shown above the digits. */
  timerName: string;
  /** True when the engine holds a configured timer. */
  armed: boolean;
  completed: boolean;
  remainingMs: number;
  progress: number;
  overtimeMs: number;
  /** True while a session cursor is loaded. */
  activeSession: boolean;
  sessionName: string;
  /** Zero-based index of the current stage. */
  stageIndex: number;
  stageCount: number;
  stageName: string;
  awaitingManual: boolean;
  transitionCountdownMs: number | null;
  message: MessageSource | null;
  /** Identity of the message, so a change can replay the animation. */
  messageKey: string;
  /** True on the final stage of a completed session. */
  sessionComplete: boolean;
}

export const EMPTY_SCENE: DisplayScene = {
  timerName: "",
  armed: false,
  completed: false,
  remainingMs: 0,
  progress: 0,
  overtimeMs: 0,
  activeSession: false,
  sessionName: "",
  stageIndex: 0,
  stageCount: 0,
  stageName: "",
  awaitingManual: false,
  transitionCountdownMs: null,
  message: null,
  messageKey: "",
  sessionComplete: false,
};

function timerFields(snapshot: TimerSnapshot | null) {
  return {
    timerName: snapshot?.name ?? "",
    armed: snapshot !== null && snapshot.timerId !== null,
    completed: snapshot?.status === "completed",
    remainingMs: snapshot?.remainingMs ?? 0,
    progress: snapshot?.progress ?? 0,
    overtimeMs: snapshot?.overtimeMs ?? 0,
  };
}

function stageMessage(message: MessageSource | null, kind: string): {
  message: MessageSource | null;
  messageKey: string;
} {
  if (!message) return { message: null, messageKey: "" };
  return { message, messageKey: `${kind}:${message.text}:${message.animation ?? "none"}` };
}

/** Builds a scene from the control window's session cursor. */
export function sceneFromSnapshot(
  snapshot: TimerSnapshot | null,
  session: SessionSnapshot | null,
  fallbackMessage: MessageSource | null,
): DisplayScene {
  const active = session !== null && session.sessionId !== null;
  const stageTotal = active ? session.stageCount : 0;
  const stageNumber = active ? session.stageIndex + 1 : 0;
  const resolved = stageMessage(
    (active ? session.activeMessage : null) ?? fallbackMessage,
    active ? "stage" : "ad-hoc",
  );

  return {
    ...timerFields(snapshot),
    activeSession: active,
    sessionName: active ? session.sessionName : "",
    stageIndex: active ? session.stageIndex : 0,
    stageCount: stageTotal,
    stageName: active ? session.stageName : "",
    awaitingManual: active ? session.awaitingManual : false,
    transitionCountdownMs: active ? session.transitionCountdownMs : null,
    message: resolved.message,
    messageKey: resolved.messageKey,
    sessionComplete:
      active && snapshot !== null && snapshot.status === "completed" && stageNumber >= stageTotal,
  };
}

/** Builds a scene from the stage the session engine pushed to the display window. */
export function sceneFromPresentation(
  snapshot: TimerSnapshot | null,
  session: SessionPresentation | null,
  fallbackMessage: MessageSource | null,
): DisplayScene {
  const active = session !== null && session.sessionId !== "";
  const stageTotal = active ? session.stageCount : 0;
  const stageNumber = active ? session.stageIndex + 1 : 0;
  const resolved = stageMessage(
    (active ? session.message : null) ?? fallbackMessage,
    active ? "stage" : "ad-hoc",
  );

  return {
    ...timerFields(snapshot),
    activeSession: active,
    sessionName: active ? session.sessionName : "",
    stageIndex: active ? session.stageIndex : 0,
    stageCount: stageTotal,
    stageName: active ? session.stageName : "",
    awaitingManual: active ? session.awaitingManual : false,
    transitionCountdownMs: active ? session.transitionCountdownMs : null,
    message: resolved.message,
    messageKey: resolved.messageKey,
    sessionComplete:
      active && snapshot !== null && snapshot.status === "completed" && stageNumber >= stageTotal,
  };
}

const ITEMS_ALIGN: Record<DisplayAlign, string> = {
  left: "items-start",
  center: "items-center",
  right: "items-end",
};

const JUSTIFY: Record<DisplayAlign, string> = {
  left: "flex-start",
  center: "center",
  right: "flex-end",
};

const TEXT_ALIGN: Record<DisplayAlign, CSSProperties["textAlign"]> = {
  left: "left",
  center: "center",
  right: "right",
};

/** Anchor insets for the overtime counter. */
function cornerStyle(corner: DisplayCorner): CSSProperties {
  const inset = "clamp(16px, 2.4cqw, 52px)";
  return {
    ...(corner.startsWith("top") ? { top: inset } : { bottom: inset }),
    ...(corner.endsWith("left") ? { left: inset } : { right: inset }),
  };
}

export interface DisplaySurfaceProps {
  style: DisplayStyle;
  scene: DisplayScene;
  timerFormat: TimerFormat;
  /** Changing this value replays the current message's entrance animation. */
  replayNonce?: number;
  className?: string;
}

/**
 * The presentation surface: one renderer for the display window and for the
 * live preview in the control window.
 *
 * It is a pure function of the engine snapshot plus the display style. It owns
 * no clock — the countdown, the progress and the overtime counter are rendered
 * straight from `scene`, so this surface can never drift from the control
 * window. It declares itself an inline-size container, which is what lets the
 * same style resolve against a 4K panel and against a preview card.
 *
 * The only thing it schedules is the *visibility* of a message whose display
 * duration has been configured; that is presentation, not timing, and it never
 * touches the authoritative timer.
 */
export function DisplaySurface({
  style,
  scene,
  timerFormat,
  replayNonce = 0,
  className,
}: DisplaySurfaceProps) {
  const { layout, timer, messageText, theme } = style;
  const gap = spacingGap(layout.spacing);

  const message = scene.message;
  const durationMs = message?.durationMs ?? 0;
  const [messageVisible, setMessageVisible] = useState(true);

  // A message with a display duration hides itself once that duration elapses.
  // Changing either the message or the replay key restarts the window.
  useEffect(() => {
    setMessageVisible(true);
    if (message === null || !(durationMs > 0)) return;
    const handle = window.setTimeout(() => setMessageVisible(false), durationMs);
    return () => window.clearTimeout(handle);
  }, [scene.messageKey, replayNonce, durationMs, message]);

  const shown = messageVisible ? message : null;

  const showSessionName = scene.activeSession && layout.showSessionInfo;
  const showStageLine = scene.activeSession && layout.showStageInfo;
  const showTimerName = scene.armed && layout.showTimerName && !scene.activeSession;
  const showHeader = showSessionName || showStageLine || showTimerName;

  const header: ReactNode = showHeader ? (
    <div className="flex w-full flex-col" style={{ gap: `calc(${gap} * 0.22)` }}>
      {showSessionName ? (
        <p
          className="max-w-full truncate font-medium tracking-[0.34em] uppercase"
          style={{ fontSize: auxiliaryFontSize(), color: theme.timerColor, opacity: 0.45 }}
        >
          {scene.sessionName}
        </p>
      ) : null}
      {showStageLine ? (
        <p
          className="max-w-full truncate font-medium tracking-[0.2em] uppercase"
          style={{ fontSize: auxiliaryFontSize(), color: theme.timerColor, opacity: 0.75 }}
        >
          Stage {scene.stageIndex + 1} / {scene.stageCount} · {scene.stageName}
        </p>
      ) : null}
      {showTimerName ? (
        <p
          className="max-w-full truncate font-medium tracking-[0.3em] uppercase"
          style={{ fontSize: auxiliaryFontSize(), color: theme.timerColor, opacity: 0.45 }}
        >
          {scene.timerName}
        </p>
      ) : null}
    </div>
  ) : null;

  const timerNode: ReactNode = (
    <div className="flex w-full" style={{ justifyContent: JUSTIFY[timer.align] }}>
      {scene.completed ? (
        <TimeIsUp className="text-center" style={{ fontSize: timeUpFontSize(timer.scale) }} />
      ) : (
        <span
          className="tabnum max-w-full leading-none"
          style={{
            fontFamily: messageFontFamily(timer.font),
            fontSize: timerFontSize(timer.scale),
            fontWeight: timer.weight,
            color: theme.timerColor,
            textAlign: TEXT_ALIGN[timer.align],
          }}
        >
          {formatDuration(scene.remainingMs, timerFormat)}
        </span>
      )}
    </div>
  );

  const banner: ReactNode =
    scene.activeSession && (scene.awaitingManual || (scene.transitionCountdownMs ?? 0) > 0) ? (
      <div
        role="status"
        className="rounded-2xl border-2 px-[0.7em] py-[0.4em] font-semibold tracking-[0.04em]"
        style={{
          fontSize: auxiliaryFontSize(),
          color: theme.progressColor,
          borderColor: theme.progressColor,
          backgroundColor: "rgba(0, 0, 0, 0.35)",
        }}
      >
        {scene.awaitingManual
          ? "Waiting for the operator"
          : `Next stage in ${formatDuration(scene.transitionCountdownMs ?? 0, "mm:ss")}`}
      </div>
    ) : null;

  const completeNote: ReactNode = scene.sessionComplete ? (
    <p
      className="font-medium tracking-[0.2em] uppercase"
      style={{ fontSize: auxiliaryFontSize(), color: theme.timerColor, opacity: 0.55 }}
    >
      Session complete
    </p>
  ) : null;

  const progressNode: ReactNode = (
    <div className="flex w-full" style={{ justifyContent: JUSTIFY[layout.align] }}>
      <div className="w-full max-w-[min(900px,72cqw)]">
        <ProgressBar
          progress={scene.progress}
          className="h-[clamp(4px,0.5cqw,10px)] bg-white/10"
          barStyle={{ backgroundColor: theme.progressColor }}
        />
      </div>
    </div>
  );

  const messageNode: ReactNode = (
    <div className="flex w-full" style={{ justifyContent: JUSTIFY[messageText.align] }}>
      <MessageSurface
        message={shown}
        placeholder=""
        replayKey={`${scene.messageKey}#${replayNonce}`}
        className="max-w-[86cqw]"
        style={{
          fontSize: messageFontSize(shown?.fontSize ?? 104, messageText.scale),
          fontWeight: messageText.weight,
          fontFamily: messageFontFamily(shown?.font ?? messageText.font),
          textAlign: TEXT_ALIGN[messageText.align],
          color: theme.messageColor || undefined,
        }}
      />
    </div>
  );

  const bands: Record<"top" | "center" | "bottom", ReactNode[]> = {
    top: [],
    center: [],
    bottom: [],
  };

  if (header) bands.top.push(header);
  bands[layout.timerPosition].push(timerNode);
  if (layout.showProgressBar) bands[layout.progressPosition].push(progressNode);
  if (layout.showMessages) bands[layout.messagePosition].push(messageNode);
  if (banner) bands.center.push(banner);
  if (completeNote) bands.bottom.push(completeNote);

  return (
    <div
      className={`relative flex h-full w-full flex-col justify-between overflow-hidden ${ITEMS_ALIGN[layout.align]} ${className ?? ""}`}
      style={{
        containerType: "inline-size",
        paddingLeft: surfacePadding(),
        paddingRight: surfacePadding(),
        paddingTop: `calc(${gap} * 0.9)`,
        paddingBottom: `calc(${gap} * 0.9)`,
        backgroundColor: theme.background,
        color: theme.timerColor,
      }}
    >
      {theme.backgroundImage ? (
        <>
          <div
            aria-hidden
            className="absolute inset-0 bg-cover bg-center"
            style={{ backgroundImage: `url(${theme.backgroundImage})` }}
          />
          {/* A scrim keeps the text readable whatever the operator chooses. */}
          <div aria-hidden className="absolute inset-0 bg-black/55" />
        </>
      ) : null}

      {(["top", "center", "bottom"] as const).map((band) => (
        <div
          key={band}
          className={`relative flex w-full flex-col ${
            band === "center" ? "flex-1 justify-center" : ""
          }`}
          style={{ gap, textAlign: TEXT_ALIGN[layout.align] }}
        >
          {bands[band]}
        </div>
      ))}

      {scene.overtimeMs > 0 ? (
        <div
          role="status"
          aria-label="Overtime"
          className="absolute rounded-[clamp(10px,1cqw,18px)] border-2 px-[0.6em] py-[0.28em] leading-none font-semibold"
          style={{
            ...cornerStyle(layout.overtimeCorner),
            fontFamily: messageFontFamily(timer.font),
            fontSize: `calc(${auxiliaryFontSize()} * 1.9)`,
            color: theme.overtimeColor,
            borderColor: theme.overtimeColor,
            backgroundColor: "rgba(0, 0, 0, 0.4)",
          }}
        >
          +{formatDuration(scene.overtimeMs, "hh:mm:ss")}
        </div>
      ) : null}
    </div>
  );
}
