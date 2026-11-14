import { CheckCircle2, Pause, Play, SkipForward, Square } from "lucide-react";
import { cn } from "../../lib/cn";
import { formatDuration } from "../../lib/time";
import { settingsStore } from "../../stores/settingsStore";
import { timerStore, pauseActiveTimer, resumeActiveTimer } from "../../stores/timerStore";
import {
  advanceSessionStage,
  sessionStore,
  stopSessionRun,
} from "../../stores/sessionStore";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { CountdownDisplay } from "../timer/CountdownDisplay";

const STATUS_LABEL: Record<string, string> = {
  running: "Running",
  awaiting: "Between stages",
  completed: "Complete",
  idle: "Idle",
};

/**
 * Session runner shown in the control window while a session is loaded.
 *
 * Every control here drives the Rust engines: pause / resume act on the
 * *running stage's* timer, so the remaining time and the stage itself are
 * preserved; "next stage" is the operator's manual transition; and stopping
 * returns both windows to the idle presentation. Nothing is counted down here.
 */
export function SessionRunner() {
  const { snapshot, busy: sessionBusy, error } = sessionStore.use();
  const { snapshot: timer, busy: timerBusy } = timerStore.use();
  const { settings } = settingsStore.use();

  if (!snapshot || snapshot.sessionId === null) return null;

  const busy = sessionBusy || timerBusy;
  const stageNumber = snapshot.stageIndex + 1;
  const completed = snapshot.status === "completed";
  const running = snapshot.status === "running";
  const awaiting = snapshot.status === "awaiting";
  const paused = timer?.status === "paused";
  const transitionMs = snapshot.transitionCountdownMs ?? 0;

  return (
    <div className="flex flex-col gap-4 rounded-[14px] border border-accent/40 bg-[#141417] p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-[11px] font-medium tracking-[0.14em] text-text-faint uppercase">
            Session
          </p>
          <p className="truncate text-[15px] font-semibold text-text">{snapshot.sessionName}</p>
          <p className="mt-0.5 truncate text-[12px] text-text-muted">
            Stage {stageNumber} / {snapshot.stageCount} · {snapshot.stageName}
          </p>
        </div>
        <Badge tone={completed ? "accent" : awaiting ? "warning" : "accent"}>
          {STATUS_LABEL[snapshot.status] ?? snapshot.status}
        </Badge>
      </div>

      <div className="flex flex-col items-center gap-3 rounded-xl border border-hairline bg-card px-4 py-4">
        {completed ? (
          <span className="flex items-center gap-2 text-[22px] font-semibold text-accent">
            <CheckCircle2 size={22} /> Session complete
          </span>
        ) : awaiting ? (
          <span className="font-mono tabnum text-[40px] leading-none font-semibold text-warning">
            {snapshot.awaitingManual
              ? "WAITING"
              : formatDuration(transitionMs, "mm:ss")}
          </span>
        ) : (
          <CountdownDisplay
            ms={timer?.remainingMs ?? 0}
            format={settings.general.defaultTimerFormat}
            className={cn("text-[42px] font-semibold", paused && "opacity-60")}
          />
        )}

        <p className="text-center text-[12px] text-text-muted">
          {completed
            ? "Every stage has finished. Close the session to return the display to idle."
            : snapshot.awaitingManual
              ? "This stage uses a manual transition — start the next stage when you are ready."
              : awaiting
                ? "The next stage starts automatically when the countdown reaches zero."
                : paused
                  ? "The stage is paused; its remaining time is preserved."
                  : "The countdown is owned by the Rust timer engine."}
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {running && !paused ? (
          <Button
            size="sm"
            icon={<Pause size={15} />}
            disabled={busy}
            onClick={() => void pauseActiveTimer()}
          >
            Pause stage
          </Button>
        ) : null}

        {running && paused ? (
          <Button
            variant="primary"
            size="sm"
            icon={<Play size={15} />}
            disabled={busy}
            onClick={() => void resumeActiveTimer()}
          >
            Resume stage
          </Button>
        ) : null}

        {!completed ? (
          <Button
            size="sm"
            icon={<SkipForward size={15} />}
            disabled={busy}
            title="Start the next stage now"
            onClick={() => void advanceSessionStage()}
          >
            Next stage
          </Button>
        ) : null}

        <Button
          variant="danger"
          size="sm"
          icon={<Square size={15} />}
          disabled={busy}
          onClick={() => void stopSessionRun()}
        >
          {completed ? "Close session" : "Stop session"}
        </Button>
      </div>

      {error ? (
        <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
