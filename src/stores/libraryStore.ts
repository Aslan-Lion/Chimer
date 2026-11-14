import type { MessagePreset, SessionDefinition, TimerDefinition, TimerPreset } from "../types";
import { createStore } from "./createStore";
import { readStorage, writeStorage } from "../lib/storage";
import { createId } from "../lib/id";

const STORAGE_KEY = "app-timer.library";

export interface HistoryRecord {
  id: string;
  timerName: string;
  originalDurationMs: number;
  elapsedMs: number;
  startedAt: number;
  endedAt: number;
  completed: boolean;
}

interface LibraryState {
  timers: TimerDefinition[];
  presets: TimerPreset[];
  messages: MessagePreset[];
  sessions: SessionDefinition[];
  history: HistoryRecord[];
}

const SEED_MESSAGES: MessagePreset[] = [
  {
    id: "msg-10-min",
    name: "Ten minutes remaining",
    text: "10 minutes remaining",
    fontSize: 104,
    color: "#F4F4F5",
    backgroundColor: null,
    position: "bottom",
    font: "sans",
    animation: "fade",
    durationMs: 8000,
    sound: "none",
    createdAt: Date.now() - 900_000,
  },
  {
    id: "msg-5-min",
    name: "Five minutes remaining",
    text: "5 minutes remaining",
    fontSize: 104,
    color: "#F4F4F5",
    backgroundColor: null,
    position: "bottom",
    font: "sans",
    animation: "fade",
    durationMs: 8000,
    sound: "chime",
    createdAt: Date.now() - 800_000,
  },
  {
    id: "msg-1-min",
    name: "One minute remaining",
    text: "1 minute remaining",
    fontSize: 118,
    color: "#FACC15",
    backgroundColor: null,
    position: "center",
    font: "mono",
    animation: "pulse",
    durationMs: 10_000,
    sound: "bell",
    createdAt: Date.now() - 700_000,
  },
  {
    id: "msg-time-up",
    name: "Time is up",
    text: "Time is up",
    fontSize: 132,
    color: "#F87171",
    backgroundColor: "#2A1414",
    position: "center",
    font: "display",
    animation: "zoom",
    durationMs: 12_000,
    sound: "alarm",
    createdAt: Date.now() - 600_000,
  },
  {
    id: "msg-submit",
    name: "Please submit your work",
    text: "Please submit your work",
    fontSize: 112,
    color: "#F4F4F5",
    backgroundColor: "#14202A",
    position: "bottom",
    font: "sans",
    animation: "slide-up",
    durationMs: 15_000,
    sound: "chime",
    createdAt: Date.now() - 500_000,
  },
];

const SEED_PRESETS: TimerPreset[] = [
  {
    id: "preset-exam",
    name: "Exam Session",
    durationMs: 90 * 60 * 1000,
    description: "Full examination block with a five minute warning.",
    defaultMessageId: "msg-5-min",
    favourite: true,
    lastUsedAt: Date.now() - 86_400_000,
    createdAt: Date.now() - 4_000_000,
  },
  {
    id: "preset-assembly",
    name: "Assembly",
    durationMs: 30 * 60 * 1000,
    description: "Morning assembly slot.",
    defaultMessageId: "msg-1-min",
    favourite: true,
    lastUsedAt: Date.now() - 172_800_000,
    createdAt: Date.now() - 3_000_000,
  },
  {
    id: "preset-workshop",
    name: "Workshop Break",
    durationMs: 15 * 60 * 1000,
    description: "Short mid-session break.",
    defaultMessageId: null,
    favourite: false,
    lastUsedAt: Date.now() - 259_200_000,
    createdAt: Date.now() - 2_000_000,
  },
  {
    id: "preset-qa",
    name: "Q&A Segment",
    durationMs: 10 * 60 * 1000,
    description: "Open question and answer segment.",
    defaultMessageId: "msg-1-min",
    favourite: true,
    lastUsedAt: Date.now() - 345_600_000,
    createdAt: Date.now() - 1_500_000,
  },
  {
    id: "preset-worship",
    name: "Worship Set",
    durationMs: 45 * 60 * 1000,
    description: "Music and worship block for services.",
    defaultMessageId: null,
    favourite: false,
    lastUsedAt: null,
    createdAt: Date.now() - 1_000_000,
  },
];

const SEED_HISTORY: HistoryRecord[] = [
  {
    id: "hist-1",
    timerName: "Exam Session",
    originalDurationMs: 90 * 60 * 1000,
    elapsedMs: 90 * 60 * 1000,
    startedAt: Date.now() - 3 * 3_600_000,
    endedAt: Date.now() - 3 * 3_600_000 + 90 * 60 * 1000,
    completed: true,
  },
  {
    id: "hist-2",
    timerName: "Workshop Break",
    originalDurationMs: 15 * 60 * 1000,
    elapsedMs: 12 * 60 * 1000,
    startedAt: Date.now() - 26 * 3_600_000,
    endedAt: Date.now() - 26 * 3_600_000 + 12 * 60 * 1000,
    completed: false,
  },
  {
    id: "hist-3",
    timerName: "Q&A Segment",
    originalDurationMs: 10 * 60 * 1000,
    elapsedMs: 10 * 60 * 1000,
    startedAt: Date.now() - 50 * 3_600_000,
    endedAt: Date.now() - 50 * 3_600_000 + 10 * 60 * 1000,
    completed: true,
  },
  {
    id: "hist-4",
    timerName: "Assembly",
    originalDurationMs: 30 * 60 * 1000,
    elapsedMs: 30 * 60 * 1000,
    startedAt: Date.now() - 74 * 3_600_000,
    endedAt: Date.now() - 74 * 3_600_000 + 30 * 60 * 1000,
    completed: true,
  },
];

const SEED_SESSIONS: SessionDefinition[] = [
  {
    id: "session-exam",
    name: "Examination Block",
    stages: [
      {
        id: "session-exam-1",
        name: "Reading time",
        durationMs: 10 * 60 * 1000,
        message: null,
        warningIntervalsMs: [60_000],
        soundEnabled: true,
        transition: "auto",
        transitionCountdownMs: 5_000,
      },
      {
        id: "session-exam-2",
        name: "Written paper",
        durationMs: 90 * 60 * 1000,
        message: null,
        warningIntervalsMs: [10 * 60 * 1000, 5 * 60 * 1000, 60_000],
        soundEnabled: true,
        transition: "manual",
        transitionCountdownMs: null,
      },
      {
        id: "session-exam-3",
        name: "Collection",
        durationMs: 5 * 60 * 1000,
        message: null,
        warningIntervalsMs: [60_000],
        soundEnabled: true,
        transition: "auto",
        transitionCountdownMs: null,
      },
    ],
  },
];

const SEED: LibraryState = {
  timers: [],
  presets: SEED_PRESETS,
  messages: SEED_MESSAGES,
  sessions: SEED_SESSIONS,
  history: SEED_HISTORY,
};

/**
 * Fills in fields a stored library predates (message typography, motion and
 * sound) so an older document keeps loading instead of leaving `undefined`
 * values in the UI.
 */
function upgradeMessage(message: MessagePreset): MessagePreset {
  return {
    ...message,
    font: message.font ?? "sans",
    animation: message.animation ?? "fade",
    sound: message.sound ?? "none",
  };
}

function normaliseLibrary(stored: Partial<LibraryState>): LibraryState {
  return {
    timers: stored.timers ?? SEED.timers,
    presets: stored.presets ?? SEED.presets,
    messages: (stored.messages ?? SEED.messages).map(upgradeMessage),
    sessions: stored.sessions ?? SEED.sessions,
    history: stored.history ?? SEED.history,
  };
}

export const libraryStore = createStore<LibraryState>(
  normaliseLibrary(readStorage<Partial<LibraryState>>(STORAGE_KEY, {})),
);

libraryStore.subscribe(() => {
  writeStorage(STORAGE_KEY, libraryStore.get());
});

/* ------------------------------------------------------------------ timers */

export function addTimer(input: Omit<TimerDefinition, "id" | "createdAt">): TimerDefinition {
  const timer: TimerDefinition = { ...input, id: createId(), createdAt: Date.now() };
  libraryStore.set((previous) => ({ timers: [timer, ...previous.timers] }));
  return timer;
}

export function removeTimer(id: string): void {
  libraryStore.set((previous) => ({
    timers: previous.timers.filter((timer) => timer.id !== id),
  }));
}

/* ----------------------------------------------------------------- presets */

export function addPreset(
  input: Omit<TimerPreset, "id" | "createdAt" | "lastUsedAt" | "favourite"> &
    Partial<Pick<TimerPreset, "favourite" | "lastUsedAt">>,
): TimerPreset {
  const preset: TimerPreset = {
    favourite: false,
    lastUsedAt: null,
    ...input,
    id: createId(),
    createdAt: Date.now(),
  };
  libraryStore.set((previous) => ({ presets: [preset, ...previous.presets] }));
  return preset;
}

export function updatePreset(id: string, patch: Partial<TimerPreset>): void {
  libraryStore.set((previous) => ({
    presets: previous.presets.map((preset) =>
      preset.id === id ? { ...preset, ...patch } : preset,
    ),
  }));
}

export function removePreset(id: string): void {
  libraryStore.set((previous) => ({
    presets: previous.presets.filter((preset) => preset.id !== id),
  }));
}

export function duplicatePreset(id: string): void {
  const source = libraryStore.get().presets.find((preset) => preset.id === id);
  if (!source) return;
  const copy: TimerPreset = {
    ...source,
    id: createId(),
    name: `${source.name} copy`,
    favourite: false,
    lastUsedAt: null,
    createdAt: Date.now(),
  };
  libraryStore.set((previous) => ({ presets: [copy, ...previous.presets] }));
}

export function togglePresetFavourite(id: string): void {
  libraryStore.set((previous) => ({
    presets: previous.presets.map((preset) =>
      preset.id === id ? { ...preset, favourite: !preset.favourite } : preset,
    ),
  }));
}

/* ---------------------------------------------------------------- messages */

export function addMessage(
  input: Omit<MessagePreset, "id" | "createdAt">,
): MessagePreset {
  const message: MessagePreset = { ...input, id: createId(), createdAt: Date.now() };
  libraryStore.set((previous) => ({ messages: [message, ...previous.messages] }));
  return message;
}

export function updateMessage(id: string, patch: Partial<MessagePreset>): void {
  libraryStore.set((previous) => ({
    messages: previous.messages.map((message) =>
      message.id === id ? { ...message, ...patch } : message,
    ),
  }));
}

export function removeMessage(id: string): void {
  libraryStore.set((previous) => ({
    messages: previous.messages.filter((message) => message.id !== id),
  }));
}

export function duplicateMessage(id: string): void {
  const source = libraryStore.get().messages.find((message) => message.id === id);
  if (!source) return;
  const copy: MessagePreset = {
    ...source,
    id: createId(),
    name: `${source.name} copy`,
    createdAt: Date.now(),
  };
  libraryStore.set((previous) => ({ messages: [copy, ...previous.messages] }));
}

/* ---------------------------------------------------------------- sessions */

export function addSession(
  input: Omit<SessionDefinition, "id">,
): SessionDefinition {
  const session: SessionDefinition = { ...input, id: createId() };
  libraryStore.set((previous) => ({ sessions: [session, ...previous.sessions] }));
  return session;
}

export function updateSession(id: string, patch: Partial<SessionDefinition>): void {
  libraryStore.set((previous) => ({
    sessions: previous.sessions.map((session) =>
      session.id === id ? { ...session, ...patch } : session,
    ),
  }));
}

export function removeSession(id: string): void {
  libraryStore.set((previous) => ({
    sessions: previous.sessions.filter((session) => session.id !== id),
  }));
}

export function duplicateSession(id: string): void {
  const source = libraryStore.get().sessions.find((session) => session.id === id);
  if (!source) return;
  const copy: SessionDefinition = {
    ...source,
    id: createId(),
    name: `${source.name} copy`,
    stages: source.stages.map((stage) => ({ ...stage, id: createId() })),
  };
  libraryStore.set((previous) => ({ sessions: [copy, ...previous.sessions] }));
}

/* ----------------------------------------------------------------- history */

export function removeHistoryRecord(id: string): void {
  libraryStore.set((previous) => ({
    history: previous.history.filter((record) => record.id !== id),
  }));
}

export function clearHistory(): void {
  libraryStore.set({ history: [] });
}
