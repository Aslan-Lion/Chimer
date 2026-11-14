/**
 * Minimal class name joiner. Kept dependency free on purpose: the interface
 * only needs conditional concatenation, not Tailwind conflict resolution.
 */
export function cn(...values: Array<string | false | null | undefined>): string {
  return values.filter((value): value is string => Boolean(value)).join(" ");
}
