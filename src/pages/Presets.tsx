import { useState } from "react";
import { LibraryBig, Plus } from "lucide-react";
import { PresetCard } from "../components/presets/PresetCard";
import { PresetFormModal } from "../components/presets/PresetFormModal";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { EmptyState } from "../components/ui/EmptyState";
import type { TimerPreset } from "../types";
import {
  duplicatePreset,
  libraryStore,
  removePreset,
  togglePresetFavourite,
  updatePreset,
} from "../stores/libraryStore";
import { startNewTimer } from "../stores/timerStore";

export default function PresetsPage() {
  const { presets, messages } = libraryStore.use();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<TimerPreset | null>(null);

  const messageName = (id: string | null): string | null =>
    id ? (messages.find((message) => message.id === id)?.name ?? null) : null;

  const openEditor = (preset: TimerPreset | null) => {
    setEditing(preset);
    setFormOpen(true);
  };

  const ordered = [...presets].sort((a, b) => {
    if (a.favourite !== b.favourite) return a.favourite ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      <div className="flex items-center justify-between gap-4">
        <p className="text-[13px] text-text-muted">
          {presets.length} preset{presets.length === 1 ? "" : "s"} stored locally.
        </p>
        <Button variant="primary" icon={<Plus size={16} />} onClick={() => openEditor(null)}>
          New preset
        </Button>
      </div>

      {ordered.length === 0 ? (
        <Card className="p-6">
          <EmptyState
            icon={<LibraryBig size={20} />}
            title="No presets yet"
            description="Presets capture a name, a duration and an optional default message so a countdown can be started in one click."
            action={
              <Button variant="primary" icon={<Plus size={16} />} onClick={() => openEditor(null)}>
                New preset
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
          {ordered.map((preset) => (
            <PresetCard
              key={preset.id}
              preset={preset}
              messageName={messageName(preset.defaultMessageId)}
              onStart={() => {
                updatePreset(preset.id, { lastUsedAt: Date.now() });
                void startNewTimer(preset.name, preset.durationMs);
              }}
              onFavourite={() => togglePresetFavourite(preset.id)}
              onDuplicate={() => duplicatePreset(preset.id)}
              onEdit={() => openEditor(preset)}
              onDelete={() => removePreset(preset.id)}
            />
          ))}
        </div>
      )}

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
