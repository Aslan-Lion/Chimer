import { Bell, BellOff, Clock3, Play, Trash2 } from "lucide-react";
import { formatDuration } from "../lib/time";
import { libraryStore, removeTimer } from "../stores/libraryStore";
import { startNewTimer } from "../stores/timerStore";
import { navigateTo, setCreateTimerOpen } from "../stores/uiStore";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { IconButton } from "../components/ui/IconButton";
import { EmptyState } from "../components/ui/EmptyState";

export default function TimersPage() {
  const { timers, messages } = libraryStore.use();

  const messageName = (id: string | null): string | null =>
    id ? (messages.find((message) => message.id === id)?.name ?? null) : null;

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      <div className="flex items-center justify-between gap-4">
        <p className="text-[13px] text-text-muted">
          {timers.length === 0
            ? "No timers configured yet."
            : `${timers.length} timer${timers.length === 1 ? "" : "s"} configured.`}
        </p>
        <Button variant="primary" icon={<Clock3 size={16} />} onClick={() => setCreateTimerOpen(true)}>
          Create timer
        </Button>
      </div>

      {timers.length === 0 ? (
        <Card className="p-6">
          <EmptyState
            icon={<Clock3 size={20} />}
            title="Your timer list is empty"
            description="Timers you create from the dashboard or here are stored locally and can be started again at any time."
            action={
              <Button
                variant="primary"
                icon={<Clock3 size={16} />}
                onClick={() => setCreateTimerOpen(true)}
              >
                Create timer
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
          {timers.map((timer) => (
            <Card key={timer.id} className="flex flex-col gap-3 p-4">
              <div className="min-w-0">
                <p className="truncate text-[14px] font-semibold text-text">{timer.name}</p>
                <p className="mt-0.5 flex items-center gap-1.5 text-[12px] text-text-muted">
                  {timer.soundEnabled ? <Bell size={13} /> : <BellOff size={13} />}
                  {timer.soundEnabled ? "Sound on completion" : "Silent"}
                  {messageName(timer.messageId) ? ` · ${messageName(timer.messageId)}` : ""}
                </p>
              </div>

              <span className="font-mono text-[26px] tabnum text-text">
                {formatDuration(timer.durationMs, "mm:ss")}
              </span>

              <div className="flex items-center justify-between gap-2 border-t border-hairline pt-3">
                <IconButton label="Delete timer" size="sm" onClick={() => removeTimer(timer.id)}>
                  <Trash2 size={15} />
                </IconButton>
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Play size={14} />}
                  onClick={() => {
                    void startNewTimer(timer.name, timer.durationMs);
                    navigateTo("dashboard");
                  }}
                >
                  Start
                </Button>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
