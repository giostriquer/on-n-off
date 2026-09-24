import {
  formatObservedAt,
  formatResetAt,
  formatResetIn,
  formatShortDate,
  formatUsedPercent,
  hasElapsed,
  parseInstant,
  usageTextColor,
} from "$lib/limitsFormat";
import type { LimitWindow, LimitsCreditsSpent, LimitsResetCredits, LimitsWorkspaceCredits, ProviderLimits } from "$lib/limitsTypes";
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
  /**
   * The card's read did not answer, so the account metadata it shows (a subscription status, a plan)
   * is what an earlier read left: the backend fills it from memory exactly then. A saved account read
   * this poll, and a card only remembered from a snapshot, both answer; `updatedAt` says how old
   * either one is.
   */
  lastKnown: boolean;
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
  return hasElapsed(resetCredits.nextExpiresAt, now) ? null : resetCredits;
}

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/**
 * Present a business workspace member's credit share as a window is presented: the reader's meter
 * (`usedPercent`, Codex's own figure), and a note saying what is left and when the share resets. Its
 * reset is checked once: past it the share has renewed, so nothing is used and all of it is left
 * again. The amounts are worded as the side notch words them (`workspace_share_wording` in
 * `side_notch/model.rs`): a reached share says "all 10,000 used" when its amounts agree and "limit
 * reached" when they show some left.
 */
export function presentWorkspaceShare(share: LimitsWorkspaceCredits, now: number): LimitWindowPresentation {
  const renewed = hasElapsed(share.resetsAt, now);
  const percent = renewed ? 0 : share.usedPercent;
  const limit = Number(share.limit);
  const used = Number(share.used);
  const amounts = renewed
    ? `${AMOUNT.format(limit)} of ${AMOUNT.format(limit)} left`
    : !share.reached
      ? `${AMOUNT.format(Math.max(limit - used, 0))} of ${AMOUNT.format(limit)} left`
      : used >= limit
        ? `all ${AMOUNT.format(limit)} used`
        : "limit reached";
  const resetDate = formatShortDate(share.resetsAt);
  const reset = renewed ? `reset ${formatAgo(share.resetsAt, now)} · ${resetDate}` : resetDate ? `resets ${resetDate}` : undefined;
  return {
    percent,
    text: formatUsedPercent(percent),
    color: usageTextColor(percent),
    note: [amounts, reset].filter(Boolean).join(" · "),
  };
}

const SPENT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 });

/**
 * What a business member spent lately, as a summary row: the last 7 days, the Codex app's default
 * view, with the last 30 days in the note. The provider's update time is left off the card, where it
 * crowded the row.
 */
export function presentCreditsSpent(spent: LimitsCreditsSpent): { value: string; note: string } {
  return {
    value: SPENT.format(spent.last7Days),
    note: `last 7 days · ${SPENT.format(spent.last30Days)} in 30 days`,
  };
}

/**
 * Whether a read observed anything about the account: quota windows, a credit balance, a
 * workspace-credit share, the credits spent lately or banked resets. `windows` lets a surface count only the windows it
 * shows. The backend's `ProviderLimitsDto::has_observations` is the same rule. A count that lapses while its card is on
 * screen still counts here until the next read, at most one poll later, drops it: only a card with
 * nothing else observed notices, and `unexpiredBankedResets` already keeps the count off it.
 */
export function hasObservations(entry: ProviderLimits, windows: LimitWindow[] = entry.windows): boolean {
  // Every current Codex read reports a reset count, usually 0; only a positive count was observed.
  return (
    windows.length > 0 ||
    entry.credits != null ||
    entry.workspaceCredits != null ||
    entry.creditsSpent != null ||
    (entry.resetCredits?.availableCount ?? 0) > 0
  );
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
    lastKnown: entry.status !== "ok",
    updatedAt: latestObservedAt === null ? null : formatObservedAt(new Date(latestObservedAt).toISOString()),
  };
}
