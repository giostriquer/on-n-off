const DAY = 86_400_000;

export const TERM_WARNING_WINDOW_MS = 7 * DAY;

export function termCountdown(remainingMs: number): { progress: number; countdown: string } {
  const days = Math.ceil(remainingMs / DAY);
  return {
    progress: Math.max(0, Math.min(1, 1 - remainingMs / TERM_WARNING_WINDOW_MS)),
    countdown: remainingMs < DAY ? "Less than 1 day remaining" : `${days} day${days === 1 ? "" : "s"} remaining`,
  };
}
