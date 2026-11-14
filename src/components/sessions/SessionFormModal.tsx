import { useEffect, useMemo, useState } from "react";
import { Play, Plus, Save } from "lucide-react";
import type { SessionDefinition } from "../../types";
import { addSession, libraryStore, updateSession } from "../../stores/libraryStore";
import { setPreviewMessage } from "../../stores/uiStore";
import { createId } from "../../lib/id";
import { formatCompactDuration, fromDurationParts } from "../../lib/time";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import type { StageDraft } from "./StageEditor";
import {
  StageEditor,
  emptyStageDraft,
  stageDraftFromStage,
  stageFromDraft,
} from "./StageEditor";

const FIELD_CLASS =
  "h-11 w-full rounded-xl border border-border bg-elevated px-3 text-sm text-text outline-none transition-colors duration-150 placeholder:text-text-faint focus:border-accent/70";

export interface SessionFormModalProps {
  open: boolean;
  /** `null` creates a new session, otherwise the session is edited in place. */
  session: SessionDefinition | null;
  onClose: () => void;
  /** Called with the saved definition when the operator asks to start it. */
  onStart: (session: SessionDefinition) => void;
}

/**
 * Session Builder.
 *
 * A session is an ordered list of stages, each of which is an ordinary
 * countdown plus presentation options. The builder only ever produces a
 * `SessionDefinition`; every countdown is run later by the Rust engine.
 */
export function SessionFormModal({ open, session, onClose, onStart }: SessionFormModalProps) {
  const { messages, presets } = libraryStore.use();
  const [name, setName] = useState("");
  const [stages, setStages] = useState<StageDraft[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setError(null);
    if (!session) {
      setName("");
      setStages([emptyStageDraft(0)]);
      return;
    }
    setName(session.name);
    setStages(
      (session.stages.length > 0 ? session.stages : [])
        .map((stage) => stageDraftFromStage(stage, messages))
        .concat(session.stages.length === 0 ? [emptyStageDraft(0)] : []),
    );
    // The library is only read when the dialog opens; editing a message while a
    // session is open is not a supported flow.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, session]);

  const totalStages = stages.length;
  const totalMs = useMemo(
    () => stages.reduce((sum, stage) => sum + fromDurationParts(stage.duration), 0),
    [stages],
  );

  const patchStage = (index: number, patch: Partial<StageDraft>) =>
    setStages((previous) =>
      previous.map((stage, position) => (position === index ? { ...stage, ...patch } : stage)),
    );

  const addStage = () => setStages((previous) => [...previous, emptyStageDraft(previous.length)]);

  const duplicateStage = (index: number) =>
    setStages((previous) => {
      const source = previous[index];
      if (!source) return previous;
      const copy: StageDraft = {
        ...source,
        id: createId(),
        name: `${source.name} copy`,
      };
      return [...previous.slice(0, index + 1), copy, ...previous.slice(index + 1)];
    });

  const moveStage = (index: number, direction: -1 | 1) =>
    setStages((previous) => {
      const target = index + direction;
      if (target < 0 || target >= previous.length) return previous;
      const next = [...previous];
      const [moved] = next.splice(index, 1);
      next.splice(target, 0, moved);
      return next;
    });

  const deleteStage = (index: number) =>
    setStages((previous) => previous.filter((_, position) => position !== index));

  const previewStage = (index: number) => {
    const stage = stages[index];
    if (!stage) return;
    setPreviewMessage(stage.messageId === "" ? null : stage.messageId);
  };

  /** Validates the whole session; returns the definition or reports the first problem. */
  const build = (): SessionDefinition | null => {
    if (name.trim().length === 0) {
      setError("Give the session a name.");
      return null;
    }
    if (stages.length === 0) {
      setError("A session needs at least one stage.");
      return null;
    }

    const built = [];
    for (const draft of stages) {
      const parsed = stageFromDraft(draft, messages);
      if (parsed.error) {
        setError(parsed.error);
        return null;
      }
      built.push(parsed.stage);
    }

    return { id: session?.id ?? createId(), name: name.trim(), stages: built };
  };

  const persist = (definition: SessionDefinition) => {
    if (session) updateSession(session.id, { name: definition.name, stages: definition.stages });
    else addSession({ name: definition.name, stages: definition.stages });
  };

  const handleSave = () => {
    setError(null);
    const definition = build();
    if (!definition) return;
    persist(definition);
    onClose();
  };

  const handleSaveAndStart = () => {
    setError(null);
    const definition = build();
    if (!definition) return;
    persist(definition);
    onClose();
    onStart(definition);
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={session ? "Edit session" : "New session"}
      description="Stages run in order. Each stage is a countdown with its own message and warnings."
      width={900}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button icon={<Save size={15} />} onClick={handleSave}>
            Save session
          </Button>
          <Button variant="primary" icon={<Play size={15} />} onClick={handleSaveAndStart}>
            Save &amp; Start
          </Button>
        </>
      }
    >
      <div className="flex max-h-[60vh] flex-col gap-5 overflow-y-auto pr-1">
        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Session name
          </span>
          <input
            type="text"
            autoFocus
            value={name}
            placeholder="e.g. Examination block"
            onChange={(event) => setName(event.currentTarget.value)}
            className={FIELD_CLASS}
          />
        </label>

        <div className="flex items-center justify-between gap-3">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Stages · {totalStages} · {formatCompactDuration(totalMs)} total
          </span>
          <Button size="sm" icon={<Plus size={14} />} onClick={addStage}>
            Add stage
          </Button>
        </div>

        {stages.length === 0 ? (
          <p className="rounded-xl border border-dashed border-border px-4 py-8 text-center text-[13px] text-text-muted">
            This session has no stages yet.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {stages.map((stage, index) => (
              <StageEditor
                key={stage.id}
                stage={stage}
                index={index}
                total={totalStages}
                messages={messages}
                presets={presets}
                onChange={(patch) => patchStage(index, patch)}
                onMoveUp={() => moveStage(index, -1)}
                onMoveDown={() => moveStage(index, 1)}
                onDuplicate={() => duplicateStage(index)}
                onDelete={() => deleteStage(index)}
                onPreview={() => previewStage(index)}
              />
            ))}
          </div>
        )}

        {error ? (
          <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
