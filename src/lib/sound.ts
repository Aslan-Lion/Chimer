import type { MessageSound } from "../types";

/**
 * Built-in notification sounds.
 *
 * The application ships no audio assets and pulls in no dependency: every
 * notification is a short, synthesised tone sequence played through the Web
 * Audio API. Rust decides *when* a cue happens; this module only turns that cue
 * into sound, so the schedule stays authoritative.
 */

export interface SoundOption {
  value: MessageSound;
  label: string;
}

export const SOUND_OPTIONS: SoundOption[] = [
  { value: "none", label: "None" },
  { value: "chime", label: "Chime" },
  { value: "bell", label: "Bell" },
  { value: "beep", label: "Beep" },
  { value: "alarm", label: "Alarm" },
];

interface Tone {
  frequency: number;
  durationMs: number;
  type: OscillatorType;
  /** Pause before this tone starts, relative to the previous one. */
  gapMs?: number;
}

const RECIPES: Record<Exclude<MessageSound, "none">, Tone[]> = {
  chime: [
    { frequency: 880, durationMs: 220, type: "sine" },
    { frequency: 1318.5, durationMs: 300, type: "sine", gapMs: 90 },
  ],
  bell: [
    { frequency: 659.25, durationMs: 320, type: "triangle" },
    { frequency: 987.77, durationMs: 380, type: "triangle", gapMs: 60 },
  ],
  beep: [{ frequency: 740, durationMs: 150, type: "square" }],
  alarm: [
    { frequency: 523.25, durationMs: 170, type: "sawtooth" },
    { frequency: 523.25, durationMs: 170, type: "sawtooth", gapMs: 110 },
    { frequency: 523.25, durationMs: 170, type: "sawtooth", gapMs: 110 },
  ],
};

let context: AudioContext | null = null;

function audioContext(): AudioContext | null {
  if (typeof window === "undefined") return null;
  const Ctor =
    window.AudioContext ??
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  if (context === null) {
    try {
      context = new Ctor();
    } catch {
      return null;
    }
  }
  return context;
}

/**
 * Plays a built-in notification sound. Unknown or `"none"` sounds are silent so
 * callers never have to guard the call. Failures degrade silently — a missing
 * audio device must never interrupt a running session.
 */
export function playSound(sound: MessageSound | null | undefined): void {
  if (!sound || sound === "none") return;
  const recipe = RECIPES[sound];
  if (!recipe) return;

  const ctx = audioContext();
  if (!ctx) return;

  if (ctx.state === "suspended") void ctx.resume();

  let start = ctx.currentTime;
  for (const tone of recipe) {
    if (tone.gapMs) start += tone.gapMs / 1000;
    const oscillator = ctx.createOscillator();
    const gain = ctx.createGain();
    const end = start + tone.durationMs / 1000;

    oscillator.type = tone.type;
    oscillator.frequency.value = tone.frequency;

    // Short attack / release so the tone cannot click.
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.exponentialRampToValueAtTime(0.22, start + 0.015);
    gain.gain.exponentialRampToValueAtTime(0.0001, end);

    oscillator.connect(gain).connect(ctx.destination);
    oscillator.start(start);
    oscillator.stop(end);

    start = end;
  }
}
