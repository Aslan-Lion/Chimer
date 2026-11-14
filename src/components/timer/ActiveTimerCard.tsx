import type { BadgeTone } from "../ui/Badge";
import { formatDuration } from "../../lib/time";
import { settingsStore } from "../../stores/settingsStore";
import {
  abortActiveTimer,
  adjustActiveTimer,
  pauseActiveTimer,
  resumeActiveTimer,
  startActiveTimer,
  stopActiveTimer,
  timerStore,
} from "../../stores/timerStore";
import { Badge } from "../ui/Badge";
import { Card, CardHeader } from "../ui/Card";
import { StatusDot } from "../ui/StatusDot";
import { CountdownDisplay } from "./CountdownDisplay";
import { ProgressBar } from "./ProgressBar";
import { TimeIsUp } from "./TimeIsUp";
import { TimerControls } from "./TimerControls";
import { TimerEmptyState } from "./TimerEmptyState";

const STATUS_LABEL: Record<string, string> = {
  idle: "Ready",
  running: "Running",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

const STATUS_TONE: Record<string, BadgeTone> = {
  idle: "neutral",
  running: "accent",
  paused: "warning",
  completed: "danger",
  stopped: "neutral",
};

export function ActiveTimerCard() {
  const { snapshot, busy, error } = timerStore.use();
  const { settings } = settingsStore.use();

  if (!snapshot || snapshot.timerId === null) return <TimerEmptyState />;

  const { status } = snapshot;
  const completed = status === "completed";
  const live = status === "running" || status === "paused";

  return (
    <Card className="flex flex-col">
      <CardHeader
        icon={<StatusDot tone={status} pulse={status === "running"} />}
        eyebrow="Active timer"
        title={snapshot.name}
        subtitle="Rendered from the Rust engine's snapshot — the display window receives the same one."
        action={<Badge tone={STATUS_TONE[status] ?? "neutral"}>{STATUS_LABEL[status] ?? status}</Badge>}
      />

      <div className="flex flex-col items-center gap-6 px-5 pt-2 pb-5">
        {completed ? (
          <TimeIsUp className="text-[46px]" />
        ) : (
          <CountdownDisplay
            ms={snapshot.remainingMs}
            format={settings.general.defaultTimerFormat}
            className="text-[76px] font-semibold"
          />
        )}

        <div className="w-full">
          <ProgressBar progress={snapshot.progress} />
        </div>

        {completed ? (
          <div className="flex w-full flex-col items-center gap-1 rounded-xl border border-[#4a2020] bg-[#2a1414] px-4 py-3">
            <span className="font-mono tabnum text-[13px] text-danger">
              +{formatDuration(snapshot.overtimeMs, "hh:mm:ss")} overtime
            </span>
          </div>
        ) : null}

        <div className="w-full border-t border-hairline pt-5">
          <TimerControls
            status={status}
            busy={busy}
            adjustmentLabel={`${settings.general.timerAdjustmentMinutes} min`}
            onAdjust={(direction) =>
              void adjustActiveTimer(
                direction * settings.general.timerAdjustmentMinutes * 60_000,
              )
            }
            onStart={status === "idle" || status === "stopped" ? () => void startActiveTimer() : undefined}
            onPause={status === "running" ? () => void pauseActiveTimer() : undefined}
            onResume={status === "paused" ? () => void resumeActiveTimer() : undefined}
            onStop={live ? () => void stopActiveTimer() : undefined}
            onAbort={() => void abortActiveTimer()}
          />
        </div>

        {error ? (
          <p className="w-full rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] leading-relaxed text-danger">
            {error}
          </p>
        ) : null}
      </div>
    </Card>
  );
}
