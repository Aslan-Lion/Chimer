export interface TimerPreset {
  id: string;
  name: string;
  durationMs: number;
  description: string;
  defaultMessageId: string | null;
  favourite: boolean;
  lastUsedAt: number | null;
  createdAt: number;
}
