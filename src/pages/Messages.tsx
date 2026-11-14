import { useState } from "react";
import { MessageSquareText, Plus } from "lucide-react";
import { MessagePresetCard } from "../components/presets/MessagePresetCard";
import { MessageFormModal } from "../components/presets/MessageFormModal";
import { MessageSurface } from "../components/display/MessageSurface";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { EmptyState } from "../components/ui/EmptyState";
import type { MessagePreset } from "../types";
import { duplicateMessage, libraryStore, removeMessage } from "../stores/libraryStore";
import { setPreviewMessage, uiStore } from "../stores/uiStore";

export default function MessagesPage() {
  const { messages } = libraryStore.use();
  const { previewMessageId } = uiStore.use();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<MessagePreset | null>(null);

  const active = messages.find((message) => message.id === previewMessageId) ?? null;

  const openEditor = (message: MessagePreset | null) => {
    setEditing(message);
    setFormOpen(true);
  };

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      <div className="flex items-center justify-between gap-4">
        <p className="text-[13px] text-text-muted">
          {messages.length} message{messages.length === 1 ? "" : "s"} available for the presentation
          display.
        </p>
        <Button variant="primary" icon={<Plus size={16} />} onClick={() => openEditor(null)}>
          New message
        </Button>
      </div>

      {active ? (
        <Card className="flex min-h-[220px] items-center justify-center bg-display p-8">
          <MessageSurface
            message={active}
            className="text-center"
            style={{ fontSize: Math.min(active.fontSize, 120) }}
          />
        </Card>
      ) : null}

      {messages.length === 0 ? (
        <Card className="p-6">
          <EmptyState
            icon={<MessageSquareText size={20} />}
            title="No messages yet"
            description="Messages appear on the presentation display during a countdown — reminders, warnings and closing instructions."
            action={
              <Button variant="primary" icon={<Plus size={16} />} onClick={() => openEditor(null)}>
                New message
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
          {messages.map((message) => (
            <MessagePresetCard
              key={message.id}
              message={message}
              previewing={message.id === previewMessageId}
              onPreview={() =>
                setPreviewMessage(message.id === previewMessageId ? null : message.id)
              }
              onEdit={() => openEditor(message)}
              onDuplicate={() => duplicateMessage(message.id)}
              onDelete={() => {
                if (message.id === previewMessageId) setPreviewMessage(null);
                removeMessage(message.id);
              }}
            />
          ))}
        </div>
      )}

      <MessageFormModal
        open={formOpen}
        message={editing}
        onClose={() => {
          setFormOpen(false);
          setEditing(null);
        }}
      />
    </div>
  );
}
