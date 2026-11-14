import { useMemo, useState } from "react";
import type { ReactNode } from "react";
import {
  ChevronRight,
  Layers,
  Pause,
  Play,
  Plus,
  RotateCcw,
  ShieldAlert,
  Square,
  Timer as TimerIcon,
  X,
} from "lucide-react";
import type { DevicePermission, RemotePreset, TimerSnapshot } from "../../types";
import { remoteStore } from "../../stores/remoteStore";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { ProgressBar } from "../timer/ProgressBar";
import { DurationInput } from "../timer/DurationInput";
import { cn } from "../../lib/cn";
import type { DurationParts } from "../../lib/time";
import {
  formatCompactDuration,
  formatDuration,
  fromDurationParts,
  toDurationParts,
} from "../../lib/time";
import { sendRemoteCommand } from "../../services/remote";
import { RemoteShell } from "./RemoteShell";

/**
 * The remote controller: the mobile-first surface a paired phone or tablet sees.
 *
 * It renders the host's snapshot and nothing else. There is no countdown here —
 * the digits are the host's `remainingMs`, redrawn every time the host pushes a
 * new revision (four times a second while running) — so this page cannot drift
 * from the desktop. Every button sends an action and waits for the host's
 * verdict; a refusal is displayed rather than optimistically applied.
 *
 * The layout adapts rather than scales: one stacked column on a phone, a wider
 * single column on a tablet, and two columns on a laptop, where the readouts sit
 * on the left and the library on the right.
 */

/** The host refuses a remotely created timer longer than this (`MAX_TIMER_DURATION_MS`). */
const MAX_DURATION_MS = 99 * 60 * 60 * 1_000;
/** The host accepts at most this many characters in a timer name (`MAX_TIMER_NAME`). */
const MAX_NAME_LENGTH = 64;

const SELECT_CLASS =
  "h-11 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[14px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

export function RemoteController() {
  const { timer, session, permissions, server, device, presets, sessions } = remoteStore.use();
  const [section, setSection] = useState<"presets" | "sessions">("presets");
  const [composerOpen, setComposerOpen] = useState(false);

  const allowed = (permission: DevicePermission) => permissions.includes(permission);

  const status = timer?.status ?? "idle";
  const hasTimer = (timer?.timerId ?? null) !== null;
  const running = status === "running";
  const paused = status === "paused";

  /** True when the host has granted this device the new create/configure right. */
  const canCreate = allowed("timerCreate");

  /** Sends one action; the store surfaces any rejection in the banner. */
  const run = (action: string, payload?: unknown) => {
    void sendRemoteCommand(action, payload);
  };

  return (
    <RemoteShell>
      {/* While the composer is open its own copy of the banner is the one on
          screen, so a refusal is always visible next to the control that
          caused it rather than scrolled away at the top of the page. */}
      {composerOpen ? null : <CommandErrorBanner />}

      <div className="flex flex-col gap-4 lg:grid lg:grid-cols-[minmax(0,1.05fr)_minmax(0,1fr)] lg:items-start lg:gap-5">
        <div className="flex flex-col gap-4 lg:gap-5">
          {/* ----------------------------------------------------- current timer */}
          <section className="rounded-2xl border border-border bg-[#101013] p-4 sm:p-5">
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-2 text-[12px] text-text-faint">
                <TimerIcon size={14} className="shrink-0" />
                <span className="truncate">{timer?.name || "No timer loaded"}</span>
              </div>
              <StatusBadge status={status} />
            </div>

            <div className="mt-3 flex items-baseline justify-center">
              <span className="tabnum max-w-full font-mono text-[clamp(30px,14vw,72px)] leading-none text-text">
                {formatDuration(timer?.remainingMs ?? 0, "hh:mm:ss")}
              </span>
            </div>

            <div className="mt-4">
              <ProgressBar
                progress={timer?.progress ?? 0}
                className="h-1.5 bg-white/10"
                barStyle={{ backgroundColor: "var(--color-accent)" }}
              />
            </div>

            {timer && timer.overtimeMs > 0 ? (
              <p className="mt-2 text-center font-mono text-[12px] text-warning">
                +{formatDuration(timer.overtimeMs, "hh:mm:ss")} overtime
              </p>
            ) : null}

            {/*
             * A phone gets two large controls per row with the destructive one
             * on its own line; from `sm` the three sit side by side, because a
             * tablet has the width for a single unambiguous row.
             */}
            <div className="mt-5 grid grid-cols-2 gap-2 sm:grid-cols-3">
              <Button
                size="lg"
                variant="primary"
                block
                icon={<Play size={16} />}
                disabled={!allowed("timerStart") || running || paused}
                onClick={() => run("timer.start")}
                title={
                  allowed("timerStart")
                    ? undefined
                    : "The host has not given this device permission to start a timer."
                }
              >
                Start
              </Button>
              {paused ? (
                <Button
                  size="lg"
                  block
                  icon={<Play size={16} />}
                  disabled={!allowed("timerResume")}
                  onClick={() => run("timer.resume")}
                >
                  Resume
                </Button>
              ) : (
                <Button
                  size="lg"
                  block
                  icon={<Pause size={16} />}
                  disabled={!allowed("timerPause") || !running}
                  onClick={() => run("timer.pause")}
                >
                  Pause
                </Button>
              )}
              <Button
                size="lg"
                variant="danger"
                block
                className="col-span-2 sm:col-span-1"
                icon={<RotateCcw size={16} />}
                disabled={!allowed("timerAbort") || !hasTimer}
                onClick={() => run("timer.abort")}
                title="Return the host to an idle timer"
              >
                Abort / Reset
              </Button>
            </div>

            <Button
              size="lg"
              block
              variant="secondary"
              className="mt-2"
              aria-expanded={composerOpen}
              icon={composerOpen ? <X size={16} /> : <Plus size={16} />}
              disabled={!canCreate && !composerOpen}
              onClick={() => setComposerOpen((open) => !open)}
              title={
                canCreate
                  ? undefined
                  : "The host has not given this device permission to create timers."
              }
            >
              {composerOpen ? "Cancel new timer" : "New timer"}
            </Button>
          </section>

          {/* ------------------------------------------------- new timer composer */}
          {composerOpen ? (
            <NewTimerComposer
              presets={presets}
              loaded={timer}
              canCreate={canCreate}
              canStart={allowed("timerStart")}
              onClose={() => setComposerOpen(false)}
            />
          ) : null}

          {/* ----------------------------------------------------------- session */}
          {session && session.sessionId !== null ? (
            <section className="rounded-2xl border border-border bg-[#101013] p-4 sm:p-5">
              <div className="flex items-center justify-between gap-3">
                <div className="flex min-w-0 items-center gap-2 text-[12px] text-text-faint">
                  <Layers size={14} className="shrink-0" />
                  <span className="truncate">{session.sessionName}</span>
                </div>
                <Badge tone={session.awaitingManual ? "warning" : "accent"}>
                  Stage {session.stageIndex + 1} / {session.stageCount}
                </Badge>
              </div>

              <p className="mt-2 text-[15px] font-medium text-text">{session.stageName}</p>
              {session.awaitingManual ? (
                <p className="mt-1 text-[12px] text-warning">Waiting for the operator to continue.</p>
              ) : null}
              {session.transitionCountdownMs !== null && session.transitionCountdownMs > 0 ? (
                <p className="mt-1 text-[12px] text-text-muted">
                  Next stage in {formatDuration(session.transitionCountdownMs, "mm:ss")}
                </p>
              ) : null}

              <div className="mt-4 grid grid-cols-2 gap-2">
                <Button
                  size="lg"
                  block
                  icon={<ChevronRight size={16} />}
                  disabled={!allowed("sessionAdvance") || !session.awaitingManual}
                  onClick={() => run("session.advance")}
                >
                  Next stage
                </Button>
                <Button
                  size="lg"
                  variant="danger"
                  block
                  icon={<Square size={15} />}
                  disabled={!allowed("sessionStop")}
                  onClick={() => run("session.stop")}
                >
                  Stop session
                </Button>
              </div>
            </section>
          ) : null}
        </div>

        {/* ------------------------------------------------------------- library */}
        <section className="rounded-2xl border border-border bg-[#101013]">
          <div className="flex gap-1 border-b border-border p-2">
            <TabButton active={section === "presets"} onClick={() => setSection("presets")}>
              Timer presets
            </TabButton>
            <TabButton active={section === "sessions"} onClick={() => setSection("sessions")}>
              Sessions
            </TabButton>
          </div>

          <div className="p-2">
            {section === "presets" ? (
              <LibraryList
                empty="No timer presets are available on the host."
                items={presets.map((preset) => ({
                  id: preset.id,
                  name: preset.name,
                  meta: formatCompactDuration(preset.durationMs),
                }))}
                disabled={!allowed("presetStart")}
                disabledHint="The host has not given this device permission to start a preset."
                onSelect={(id) => run("preset.start", { presetId: id })}
              />
            ) : (
              <LibraryList
                empty="No sessions are available on the host."
                items={sessions.map((session) => ({
                  id: session.id,
                  name: session.name,
                  meta: `${session.stageCount} stage${session.stageCount === 1 ? "" : "s"}`,
                }))}
                disabled={!allowed("sessionStart")}
                disabledHint="The host has not given this device permission to start a session."
                onSelect={(id) => run("session.start", { sessionId: id })}
              />
            )}
          </div>
        </section>
      </div>

      <p className="px-1 pb-2 text-center text-[11px] text-text-faint">
        {device?.name ? `${device.name} · ` : ""}
        {server?.name ?? "app.timer"} {server?.version ? `v${server.version}` : ""}
      </p>
    </RemoteShell>
  );
}

/**
 * Configure a brand-new timer on the host from the phone.
 *
 * Nothing here counts or starts anything: `Configure` sends `timer.create` and
 * `Start` asks the host to run it. The host's own engine owns the definition,
 * the countdown and the one-active-timer rule — this form only shapes a valid
 * request, mirrors the host's validation so an impossible draft never leaves the
 * device, and shows the host's verdict when it disagrees.
 */
function NewTimerComposer({
  presets,
  loaded,
  canCreate,
  canStart,
  onClose,
}: {
  presets: RemotePreset[];
  loaded: TimerSnapshot | null;
  canCreate: boolean;
  canStart: boolean;
  onClose: () => void;
}) {
  const [presetId, setPresetId] = useState("");
  const [name, setName] = useState("");
  const [duration, setDuration] = useState<DurationParts>({ hours: 0, minutes: 25, seconds: 0 });
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const totalMs = useMemo(() => fromDurationParts(duration), [duration]);
  const trimmed = name.trim();

  /** True when the host already holds exactly this definition, so no create is needed. */
  const matchesLoaded =
    loaded !== null &&
    loaded.timerId !== null &&
    loaded.name === trimmed &&
    loaded.durationMs === totalMs;

  /** A stored preset is a starting point: it fills the form and stays editable. */
  const applyPreset = (id: string) => {
    setPresetId(id);
    setError(null);
    setNote(null);
    const preset = presets.find((entry) => entry.id === id);
    if (!preset) return;
    setName(preset.name);
    setDuration(toDurationParts(preset.durationMs));
  };

  /** Mirrors the host's rules so a rejected request is never sent blindly. */
  const validate = (): string | null => {
    if (trimmed.length === 0) return "Give the timer a name before continuing.";
    if (totalMs <= 0) return "A timer needs a duration greater than zero.";
    if (totalMs > MAX_DURATION_MS) return "A timer can be at most 99 hours.";
    return null;
  };

  const configure = async () => {
    const problem = validate();
    if (problem) {
      setError(problem);
      setNote(null);
      return;
    }
    setError(null);
    setNote(null);
    setBusy(true);
    const outcome = await sendRemoteCommand("timer.create", { name: trimmed, durationMs: totalMs });
    setBusy(false);
    // A refusal (including the host's `timer-active`) is already on screen in
    // the banner; a success is confirmed here because the timer card above
    // updates from the host's own broadcast.
    if (!outcome.ok) return;
    setNote("Configured on the host. Review it above, then press Start.");
  };

  const start = async () => {
    const problem = validate();
    if (problem) {
      setError(problem);
      setNote(null);
      return;
    }
    setError(null);
    setNote(null);
    setBusy(true);
    // Only a definition the host does not already hold is sent; an unchanged
    // one is started in place, which keeps a deliberate Configure followed by
    // Start from creating the timer twice.
    if (!matchesLoaded) {
      const created = await sendRemoteCommand("timer.create", { name: trimmed, durationMs: totalMs });
      if (!created.ok) {
        setBusy(false);
        return;
      }
    }
    const started = await sendRemoteCommand("timer.start");
    setBusy(false);
    if (!started.ok) return;
    onClose();
  };

  const configureDisabled = busy || !canCreate;
  const startDisabled = busy || !canStart || (!matchesLoaded && !canCreate);

  return (
    <section className="rounded-2xl border border-border bg-[#101013] p-4 sm:p-5">
      <div className="flex items-center gap-2 text-[13px] font-medium text-text">
        <Plus size={15} className="text-accent" />
        New timer
      </div>
      <p className="mt-1 text-[12px] leading-relaxed text-text-muted">
        Configure a countdown on the host, review it, then start it. The host owns the clock; this
        page only asks.
      </p>

      <div className="mt-3">
        <CommandErrorBanner />
      </div>

      {/*
       * `[&_input]:text-[16px]` is deliberate: iOS Safari zooms the whole page
       * when a text field smaller than 16px takes focus, which would shift the
       * layout out from under the operator every time they tap a duration box.
       */}
      <div className="mt-3 flex flex-col gap-4 [&_input]:text-[16px]">
        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Start from a preset
          </span>
          <select
            value={presetId}
            onChange={(event) => applyPreset(event.currentTarget.value)}
            className={SELECT_CLASS}
          >
            <option value="">Blank timer</option>
            {presets.map((preset) => (
              <option key={preset.id} value={preset.id}>
                {preset.name} · {formatCompactDuration(preset.durationMs)}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Timer name
          </span>
          <input
            type="text"
            value={name}
            maxLength={MAX_NAME_LENGTH}
            placeholder="e.g. Morning examination"
            onChange={(event) => {
              setName(event.currentTarget.value);
              setError(null);
            }}
            className="h-11 w-full rounded-xl border border-border bg-elevated px-3 text-[14px] text-text outline-none transition-colors duration-150 placeholder:text-text-faint focus:border-accent/70"
          />
        </label>

        <DurationInput value={duration} onChange={setDuration} />

        {/* The duration the operator is about to ask for, before anything is sent. */}
        <div className="rounded-xl border border-border bg-[#141417] px-3.5 py-3">
          <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Review
          </span>
          <div className="mt-1 flex items-baseline justify-between gap-3">
            <span className="min-w-0 truncate text-[13px] text-text">
              {trimmed || "Untitled timer"}
            </span>
            <span className="tabnum shrink-0 font-mono text-[15px] text-accent">
              {formatDuration(totalMs, "hh:mm:ss")}
            </span>
          </div>
        </div>

        {error ? (
          <p className="rounded-lg border border-[#4a2020] bg-[#2a1414] px-3 py-2 text-[12px] text-danger">
            {error}
          </p>
        ) : null}
        {note ? (
          <p className="rounded-lg border border-[#1f4531] bg-[#10231a] px-3 py-2 text-[12px] text-success">
            {note}
          </p>
        ) : null}

        {/* Stacked on a phone, side by side once there is room for both. */}
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <Button
            size="lg"
            block
            icon={<TimerIcon size={16} />}
            disabled={configureDisabled}
            onClick={() => void configure()}
          >
            Configure
          </Button>
          <Button
            size="lg"
            variant="primary"
            block
            icon={<Play size={16} />}
            disabled={startDisabled}
            onClick={() => void start()}
          >
            Start
          </Button>
        </div>

        {!canCreate ? (
          <p className="text-[11px] text-text-faint">
            The host has not given this device permission to create a timer.
          </p>
        ) : !matchesLoaded ? (
          <p className="text-[11px] leading-relaxed text-text-faint">
            Start configures the timer on the host first, then runs it. The host refuses a new timer
            while one is already running, paused or showing overtime.
          </p>
        ) : (
          <p className="text-[11px] text-text-faint">
            The host already holds this exact timer and will start it in place.
          </p>
        )}
      </div>
    </section>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "h-10 flex-1 rounded-lg text-[13px] font-medium transition-colors",
        active ? "bg-[#1c1c20] text-text" : "text-text-muted hover:text-text",
      )}
    >
      {children}
    </button>
  );
}

function StatusBadge({ status }: { status: string }) {
  if (status === "running") return <Badge tone="success">Running</Badge>;
  if (status === "paused") return <Badge tone="warning">Paused</Badge>;
  if (status === "completed") return <Badge tone="danger">Time is up</Badge>;
  if (status === "stopped") return <Badge tone="neutral">Stopped</Badge>;
  return <Badge tone="neutral">Idle</Badge>;
}

/** The last command rejection, shown until the next action clears it. */
function CommandErrorBanner() {
  const { commandError } = remoteStore.use();
  if (!commandError) return null;
  return (
    <div className="flex items-start gap-2.5 rounded-xl border border-[#4a2020] bg-[#2a1414] px-3.5 py-3 text-[12px] leading-relaxed">
      <ShieldAlert size={15} className="mt-0.5 shrink-0 text-danger" />
      <div className="min-w-0">
        <p className="text-danger">{commandError.message}</p>
        <p className="mt-0.5 font-mono text-[11px] text-text-faint">{commandError.code}</p>
      </div>
    </div>
  );
}

interface LibraryItem {
  id: string;
  name: string;
  meta: string;
}

/** A tappable list of presets or sessions the host actually holds. */
function LibraryList({
  items,
  empty,
  disabled,
  disabledHint,
  onSelect,
}: {
  items: LibraryItem[];
  empty: string;
  disabled: boolean;
  disabledHint: string;
  onSelect: (id: string) => void;
}) {
  if (items.length === 0) {
    return <p className="px-3 py-6 text-center text-[12px] text-text-muted">{empty}</p>;
  }
  return (
    <>
      <ul className="flex flex-col">
        {items.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              disabled={disabled}
              onClick={() => onSelect(item.id)}
              className={cn(
                "flex w-full items-center justify-between gap-3 rounded-xl px-3 py-3 text-left transition-colors",
                disabled ? "opacity-40" : "hover:bg-[#18181b]",
              )}
            >
              <span className="min-w-0">
                <span className="block truncate text-[14px] text-text">{item.name}</span>
                <span className="block text-[12px] text-text-faint">{item.meta}</span>
              </span>
              <Play
                size={15}
                className={cn("shrink-0", disabled ? "text-text-faint" : "text-accent")}
              />
            </button>
          </li>
        ))}
      </ul>
      {disabled ? <p className="px-3 pt-2 pb-3 text-[11px] text-text-faint">{disabledHint}</p> : null}
    </>
  );
}
