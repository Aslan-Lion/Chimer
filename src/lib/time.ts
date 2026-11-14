export type TimerFormat = "hh:mm:ss" | "mm:ss";

const pad = (value: number): string => value.toString().padStart(2, "0");

export interface DurationParts {
  hours: number;
  minutes: number;
  seconds: number;
}

/** Split a millisecond duration into whole hours / minutes / seconds. */
export function toDurationParts(ms: number): DurationParts {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  return {
    hours: Math.floor(totalSeconds / 3600),
    minutes: Math.floor((totalSeconds % 3600) / 60),
    seconds: totalSeconds % 60,
  };
}

/** Combine hours / minutes / seconds into milliseconds. */
export function fromDurationParts(parts: DurationParts): number {
  const { hours, minutes, seconds } = parts;
  return ((hours * 3600) + (minutes * 60) + seconds) * 1000;
}

/** Render a duration as `HH:MM:SS` (or `MM:SS` when hours are zero and requested). */
export function formatDuration(ms: number, format: TimerFormat = "hh:mm:ss"): string {
  const { hours, minutes, seconds } = toDurationParts(ms);
  if (format === "mm:ss" && hours === 0) {
    return `${pad(minutes)}:${pad(seconds)}`;
  }
  return `${pad(hours)}:${pad(minutes)}:${pad(seconds)}`;
}

/** Compact, human readable duration such as `25 min` or `1 h 30 min`. */
export function formatCompactDuration(ms: number): string {
  const { hours, minutes } = toDurationParts(ms);
  const chunks: string[] = [];
  if (hours > 0) chunks.push(`${hours} h`);
  if (minutes > 0) chunks.push(`${minutes} min`);
  if (chunks.length === 0) chunks.push(`${toDurationParts(ms).seconds} s`);
  return chunks.join(" ");
}

/** Local date + time rendering used by the history table. */
export function formatDateTime(timestamp: number): string {
  return new Date(timestamp).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatClockTime(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
