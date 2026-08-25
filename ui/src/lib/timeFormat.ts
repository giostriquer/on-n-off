/** Relative-time wording shared by the screens (pure). */

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * "just now" / "5m ago" / "2h ago" / "3d ago" since `iso`, in the coarsest unit that fits. A
 * future instant reads "just now" (clock skew, never a countdown); unreadable input is empty.
 */
export function formatAgo(iso: string | null | undefined, nowMs: number): string {
  const at = iso ? Date.parse(iso) : Number.NaN;
  if (Number.isNaN(at)) return "";
  const elapsed = Math.max(0, nowMs - at);
  if (elapsed < MINUTE_MS) return "just now";
  if (elapsed < HOUR_MS) return `${Math.floor(elapsed / MINUTE_MS)}m ago`;
  if (elapsed < DAY_MS) return `${Math.floor(elapsed / HOUR_MS)}h ago`;
  return `${Math.floor(elapsed / DAY_MS)}d ago`;
}
