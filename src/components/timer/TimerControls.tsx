import { Pause, Play, RotateCcw, Square } from "lucide-react";
import type { TimerStatus } from "../../types";
import { Button } from "../ui/Button";

export interface TimerControlsProps {
  status: TimerStatus;
  /**
   * Handlers call the Rust timer engine, which is the only authority that may
   * change the timer. A control whose handler is absent renders disabled, so the
   * interface never offers an action the engine cannot perform.
   */
  onStart?: () => void;
  onPause?: () => void;
  onResume?: () => void;
  onStop?: () => void;
  /** Aborts the timer: the engine discards it and returns to its idle state. */
  onAbort?: () => void;
  /** Holds every control while a command is in flight. */
  busy?: boolean;
}

export function TimerControls({
  status,
  onStart,
  onPause,
  onResume,
  onStop,
  onAbort,
  busy = false,
}: TimerControlsProps) {
  const running = status === "running";
  const paused = status === "paused";

  return (
    <div className="flex flex-wrap items-center gap-2">
      {running ? (
        <Button
          variant="primary"
          icon={<Pause size={16} />}
          disabled={busy || !onPause}
          onClick={onPause}
        >
          Pause
        </Button>
      ) : (
        <Button
          variant="primary"
          icon={<Play size={16} />}
          disabled={busy || (!onStart && !onResume)}
          onClick={paused ? onResume : onStart}
          title={status === "completed" ? "Reset the timer before starting it again" : undefined}
        >
          {paused ? "Resume" : "Start"}
        </Button>
      )}

      <div className="mx-1 h-6 w-px bg-hairline" />

      <Button
        icon={<Square size={14} />}
        disabled={busy || !onStop}
        onClick={onStop}
        title="Stop timer"
      >
        Stop
      </Button>

      <Button
        variant="danger"
        size="md"
        icon={<RotateCcw size={15} />}
        disabled={busy || !onAbort}
        onClick={onAbort}
        title="Abort the timer and return the display to 00:00"
      >
        Abort / Reset
      </Button>
    </div>
  );
}
