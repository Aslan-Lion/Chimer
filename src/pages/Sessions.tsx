import { useState } from "react";
import { ListOrdered, Plus } from "lucide-react";
import type { SessionDefinition, SessionStage } from "../types";
import { SessionCard } from "../components/sessions/SessionCard";
import { SessionFormModal } from "../components/sessions/SessionFormModal";
import { SessionRunner } from "../components/sessions/SessionRunner";
import { resolveStageMessage } from "../components/sessions/StageEditor";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { EmptyState } from "../components/ui/EmptyState";
import {
  duplicateSession,
  libraryStore,
  removeSession,
} from "../stores/libraryStore";
import { sessionStore, startSession } from "../stores/sessionStore";

export default function SessionsPage() {
  const { sessions, messages } = libraryStore.use();
  const { snapshot } = sessionStore.use();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<SessionDefinition | null>(null);

  /**
   * Re-resolves every stage message against the current library before the
   * session is handed to Rust.
   *
   * The stored definition carries a snapshot of each message so Rust can present
   * a stage on its own; re-resolving here means an edited message is reflected
   * the next time the session is started, without the frontend ever running the
   * countdown itself.
   */
  const resolveMessages = (definition: SessionDefinition): SessionDefinition => ({
    ...definition,
    stages: definition.stages.map((stage): SessionStage => {
      const messageId = stage.message?.id;
      const message = messageId
        ? (messages.find((entry) => entry.id === messageId) ?? null)
        : null;
      return { ...stage, message: message ? resolveStageMessage(message) : stage.message };
    }),
  });

  const openEditor = (session: SessionDefinition | null) => {
    setEditing(session);
    setFormOpen(true);
  };

  const runningId = snapshot?.sessionId ?? null;

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      <SessionRunner />

      <div className="flex items-center justify-between gap-4">
        <p className="text-[13px] text-text-muted">
          {sessions.length} session{sessions.length === 1 ? "" : "s"} stored locally.
        </p>
        <Button variant="primary" icon={<Plus size={16} />} onClick={() => openEditor(null)}>
          New session
        </Button>
      </div>

      {sessions.length === 0 ? (
        <Card className="p-6">
          <EmptyState
            icon={<ListOrdered size={20} />}
            title="No sessions yet"
            description="A session runs several countdowns one after another — reading time, a written paper, collection — each with its own message and warnings."
            action={
              <Button variant="primary" icon={<Plus size={16} />} onClick={() => openEditor(null)}>
                New session
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
          {sessions.map((session) => (
            <SessionCard
              key={session.id}
              session={session}
              running={session.id === runningId}
              onStart={() => void startSession(resolveMessages(session))}
              onEdit={() => openEditor(session)}
              onDuplicate={() => duplicateSession(session.id)}
              onDelete={() => removeSession(session.id)}
            />
          ))}
        </div>
      )}

      <SessionFormModal
        open={formOpen}
        session={editing}
        onClose={() => {
          setFormOpen(false);
          setEditing(null);
        }}
        onStart={(session) => void startSession(resolveMessages(session))}
      />
    </div>
  );
}
