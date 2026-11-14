import { History as HistoryIcon, Trash2 } from "lucide-react";
import { formatDateTime, formatDuration } from "../lib/time";
import { clearHistory, libraryStore, removeHistoryRecord } from "../stores/libraryStore";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Card, CardHeader } from "../components/ui/Card";
import { EmptyState } from "../components/ui/EmptyState";
import { IconButton } from "../components/ui/IconButton";

export default function HistoryPage() {
  const { history } = libraryStore.use();
  const ordered = [...history].sort((a, b) => b.startedAt - a.startedAt);

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      <Card>
        <CardHeader
          icon={<HistoryIcon size={16} />}
          eyebrow="Sessions"
          title="Timer history"
          subtitle="Stopping a timer keeps its record — nothing is deleted implicitly."
          action={
            ordered.length > 0 ? (
              <Button size="sm" icon={<Trash2 size={14} />} onClick={clearHistory}>
                Clear history
              </Button>
            ) : null
          }
        />

        <div className="px-5 pb-5">
          {ordered.length === 0 ? (
            <EmptyState
              icon={<HistoryIcon size={20} />}
              title="No sessions recorded"
              description="Completed and stopped timers appear here with their planned and actual durations."
            />
          ) : (
            <div className="overflow-hidden rounded-xl border border-border">
              <div className="grid grid-cols-[minmax(0,2fr)_repeat(4,minmax(0,1fr))_84px] gap-3 border-b border-border bg-[#141417] px-4 py-2.5 text-[11px] font-semibold tracking-[0.1em] text-text-faint uppercase">
                <span>Timer</span>
                <span>Planned</span>
                <span>Elapsed</span>
                <span>Started</span>
                <span>Status</span>
                <span className="text-right">Actions</span>
              </div>

              {ordered.map((record) => (
                <div
                  key={record.id}
                  className="grid grid-cols-[minmax(0,2fr)_repeat(4,minmax(0,1fr))_84px] items-center gap-3 border-b border-hairline px-4 py-3 text-[13px] last:border-b-0"
                >
                  <span className="truncate font-medium text-text">{record.timerName}</span>
                  <span className="font-mono tabnum text-text-muted">
                    {formatDuration(record.originalDurationMs, "mm:ss")}
                  </span>
                  <span className="font-mono tabnum text-text-muted">
                    {formatDuration(record.elapsedMs, "mm:ss")}
                  </span>
                  <span className="text-[12px] text-text-muted">
                    {formatDateTime(record.startedAt)}
                  </span>
                  <span>
                    <Badge tone={record.completed ? "success" : "warning"}>
                      {record.completed ? "Completed" : "Stopped"}
                    </Badge>
                  </span>
                  <span className="flex justify-end">
                    <IconButton
                      label="Delete record"
                      size="sm"
                      onClick={() => removeHistoryRecord(record.id)}
                    >
                      <Trash2 size={15} />
                    </IconButton>
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}
