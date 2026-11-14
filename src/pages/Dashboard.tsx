import { useState } from "react";
import { MessageSquareText, Sparkles } from "lucide-react";
import { ActiveTimerCard } from "../components/timer/ActiveTimerCard";
import { QuickActions } from "../components/timer/QuickActions";
import { SessionRunner } from "../components/sessions/SessionRunner";
import { PresetCard } from "../components/presets/PresetCard";
import { PresetFormModal } from "../components/presets/PresetFormModal";
import { DisplayWindowStatusCard } from "../components/display/DisplayWindowStatusCard";
import { Card, CardHeader } from "../components/ui/Card";
import { EmptyState } from "../components/ui/EmptyState";
import { Button } from "../components/ui/Button";
import type { TimerPreset } from "../types";
import {
  duplicatePreset,
  libraryStore,
  removePreset,
  togglePresetFavourite,
  updatePreset,
} from "../stores/libraryStore";
import { startNewTimer } from "../stores/timerStore";
import { navigateTo } from "../stores/uiStore";

export default function DashboardPage() {
  const { presets, messages } = libraryStore.use();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<TimerPreset | null>(null);

  const featured = [...presets]
    .sort((a, b) => {
      if (a.favourite !== b.favourite) return a.favourite ? -1 : 1;
      return (b.lastUsedAt ?? b.createdAt) - (a.lastUsedAt ?? a.createdAt);
    })
    .slice(0, 5);

  const messageName = (id: string | null): string | null =>
    id ? (messages.find((message) => message.id === id)?.name ?? null) : null;

  const startPreset = (preset: TimerPreset) => {
    updatePreset(preset.id, { lastUsedAt: Date.now() });
    void startNewTimer(preset.name, preset.durationMs);
  };

  const openEditor = (preset: TimerPreset) => {
    setEditing(preset);
    setFormOpen(true);
  };

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      <div className="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1.85fr)_minmax(0,1fr)]">
        <div className="flex flex-col gap-5">
          <SessionRunner />
          <ActiveTimerCard />

          <Card>
            <CardHeader
              eyebrow="Quick actions"
              title="Shortcuts"
              subtitle="Common actions for this workspace."
            />
            <div className="px-5 pb-5">
              <QuickActions />
            </div>
          </Card>
        </div>

        <div className="flex flex-col gap-5">
          <DisplayWindowStatusCard />

          <Card>
            <CardHeader
              icon={<MessageSquareText size={16} />}
              eyebrow="Library"
              title="Message presets"
              subtitle="Frequently used presentation messages."
            />
            <div className="flex flex-col gap-2 px-5 pb-5">
              {messages.slice(0, 5).map((message) => (
                <button
                  key={message.id}
                  type="button"
                  onClick={() => navigateTo("messages")}
                  className="flex flex-col gap-1 rounded-xl border border-border bg-elevated px-3.5 py-2.5 text-left transition-colors duration-150 hover:border-border-strong"
                >
                  <span className="flex items-center justify-between gap-3">
                    <span className="truncate text-[13px] font-medium text-text">
                      {message.name}
                    </span>
                    <span className="shrink-0 text-[11px] text-text-faint">
                      {message.position} · {Math.round(message.durationMs / 1000)}s
                    </span>
                  </span>
                  <span className="truncate text-[12px] text-text-muted">{message.text}</span>
                </button>
              ))}
            </div>
          </Card>
        </div>
      </div>

      <Card>
        <CardHeader
          icon={<Sparkles size={16} />}
          eyebrow="Library"
          title="Timer presets"
          subtitle="Favourites first, then the most recently used countdowns."
        />
        <div className="px-5 pb-5">
          {featured.length === 0 ? (
            <EmptyState
              icon={<Sparkles size={20} />}
              title="No presets yet"
              description="Save a timer as a preset to reuse it in examinations, services and presentations."
              action={
                <Button
                  variant="primary"
                  onClick={() => {
                    setEditing(null);
                    setFormOpen(true);
                  }}
                >
                  New preset
                </Button>
              }
            />
          ) : (
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
              {featured.map((preset) => (
                <PresetCard
                  key={preset.id}
                  preset={preset}
                  messageName={messageName(preset.defaultMessageId)}
                  onStart={() => startPreset(preset)}
                  onFavourite={() => togglePresetFavourite(preset.id)}
                  onDuplicate={() => duplicatePreset(preset.id)}
                  onEdit={() => openEditor(preset)}
                  onDelete={() => removePreset(preset.id)}
                />
              ))}
            </div>
          )}
        </div>
      </Card>

      <PresetFormModal
        open={formOpen}
        preset={editing}
        onClose={() => {
          setFormOpen(false);
          setEditing(null);
        }}
      />
    </div>
  );
}
