import {
  formatObservedAt,
  formatResetAt,
  formatResetIn,
  formatUsedPercent,
  hasElapsed,
  parseInstant,
  usageTextColor,
} from "$lib/limitsFormat";
import type { LimitWindow, LimitsResetCredits, ProviderLimits } from "$lib/limitsTypes";
import { formatAgo } from "$lib/timeFormat";

export type LimitWindowPresentation = {
  percent: number;
  /** The value slot: the window's percentage, which a passed reset puts back at zero. */
  text: string;
  /** Colour for the value slot; undefined leaves the default ink. */
  color: string | undefined;
  note: string;
};

export type LimitAccountPresentation = {
  message: string | null;
  refreshPaused: boolean;
  savedRefreshDetail: string | null;
  remembered: boolean;
  updatedAt: string | null;
};

const HIDDEN_CODEX_LIMIT_BUCKETS = ["base_model_inference", "codex_bengalfox"];
const HIDDEN_CODEX_LIMIT_LABELS = ["gpt-reserve", "gpt-5.3-codex-spark"];

/** Keep provider-owned internal and retired preview buckets out of both Limits surfaces. */
export function visibleLimitWindows(entry: ProviderLimits): LimitWindow[] {
  if (entry.provider !== "codex") return entry.windows;
  return entry.windows.filter((window) => {
    const label = window.label.split("·").at(-1)?.trim().toLowerCase();
    return (
      !HIDDEN_CODEX_LIMIT_LABELS.includes(label ?? "") &&
      !HIDDEN_CODEX_LIMIT_BUCKETS.some((bucket) => {
        const id = `extra:${bucket}`;
        return window.id === id || window.id.startsWith(`${id}:`);
      })
    );
  });
}

/**
 * Present one independently observed quota window. An observation describes only the cycle it was
 * taken in: once the window's own reset passes, the figure it carried belongs to a spent cycle and
 * the quota it measured has renewed. Account status cannot keep a prior cycle's number current.
 *
 * So a passed reset puts the window back at zero — which is where the provider starts it — and the
 * note says when that happened. The spent cycle's number is not carried forward into the new one,
 * even as a footnote: a remembered account reads as renewed, not as its old high-water mark.
 */
export function presentLimitWindow(window: LimitWindow, now: number): LimitWindowPresentation {
  const resetAt = formatResetAt(window.resetsAt);
  const elapsed = hasElapsed(window.resetsAt, now);
  const usedPercent = elapsed ? 0 : window.usedPercent;
  return {
    percent: usedPercent,
    text: formatUsedPercent(usedPercent),
    color: usageTextColor(usedPercent),
    note: elapsed ? elapsedNote(window, now, resetAt) : pendingNote(formatResetIn(window.resetsAt, now), resetAt),
  };
}

/** "reset 1h ago · Mon 20:34": when the quota renewed, not what it held before it did. */
function elapsedNote(window: LimitWindow, now: number, resetAt: string): string {
  return `reset ${formatAgo(window.resetsAt, now)} · ${resetAt}`;
}

/** "resets in 6d 12h · Mon 11:00"; empty when the provider reported no reset. */
function pendingNote(resetIn: string, resetAt: string): string {
  return resetIn ? `resets in ${resetIn}${resetAt ? ` · ${resetAt}` : ""}` : "";
}

/** The 5-hour and weekly windows: the two that gate every request. Model-specific buckets do not. */
function mainWindows(entry: ProviderLimits): LimitWindow[] {
  return entry.windows.filter((window) => window.kind === "session" || window.kind === "weekly");
}

/**
 * How much of the account's usage is left, as a percentage: what its most-used main window (5-hour
 * or weekly) has left, with a window whose reset has passed counted as renewed. Model-specific
 * buckets do not count. `null` when no main window is known.
 */
export function usageLeft(entry: ProviderLimits, now: number): number | null {
  const used = mainWindows(entry).map((window) => presentLimitWindow(window, now).percent);
  return used.length ? 100 - Math.max(...used) : null;
}

/**
 * When an account that is out of usage has it again: the instant the last of its full main windows
 * resets, `Infinity` when one of them reports no reset. Meaningful only when `usageLeft` is zero;
 * with no full main window it is `-Infinity`, which is to say already.
 */
export function usableAgainAt(entry: ProviderLimits, now: number): number {
  return Math.max(
    ...mainWindows(entry)
      .filter((window) => presentLimitWindow(window, now).percent >= 100)
      .map((window) => parseInstant(window.resetsAt) ?? Infinity),
  );
}

/**
 * The banked resets a card can still show: a positive count whose soonest known expiry is ahead of
 * `now`. Once that expiry passes, at least one reset has lapsed and what is left is not known, so the
 * count stays off the card until a read answers again. The backend drops such a count from
 * remembered snapshots by the same rule.
 */
export function unexpiredBankedResets(resetCredits: LimitsResetCredits | null | undefined, now: number): LimitsResetCredits | null {
  if (!resetCredits || resetCredits.availableCount <= 0) return null;
  const expiresAt = parseInstant(resetCredits.nextExpiresAt);
  return expiresAt !== null && expiresAt <= now ? null : resetCredits;
}

/**
 * Whether a read observed anything about the account: quota windows, a credit balance or banked
 * resets. `windows` lets a surface count only the windows it shows. The backend's
 * `ProviderLimitsDto::has_observations` is the same rule.
 */
export function hasObservations(entry: ProviderLimits, windows: LimitWindow[] = entry.windows): boolean {
  // Every current Codex read reports a reset count, usually 0; only a positive count was observed.
  return windows.length > 0 || entry.credits != null || (entry.resetCredits?.availableCount ?? 0) > 0;
}

export function presentLimitAccount(entry: ProviderLimits, fallbackMessage: string): LimitAccountPresentation {
  const windows = visibleLimitWindows(entry);
  const observed = hasObservations(entry, windows);
  const latestObservedAt = windows.reduce<number | null>((latest, window) => {
    const observedAt = Date.parse(window.observedAt);
    if (Number.isNaN(observedAt)) return latest;
    return latest === null ? observedAt : Math.max(latest, observedAt);
  }, null);
  const savedRefreshPaused = !entry.currentAccount && observed && (entry.status === "failed" || entry.status === "unauthenticated");
  const detail = entry.message ?? fallbackMessage;
  return {
    message: entry.status === "ok" || savedRefreshPaused ? null : detail,
    savedRefreshDetail: savedRefreshPaused ? detail : null,
    refreshPaused: entry.currentAccount && entry.status !== "ok" && observed,
    remembered: !entry.currentAccount && observed,
    updatedAt: latestObservedAt === null ? null : formatObservedAt(new Date(latestObservedAt).toISOString()),
  };
}
