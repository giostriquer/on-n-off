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
  text: string;
  color: string | undefined;
  note: string;
};

export type CardStatus = { kind: "savedRefresh"; detail: string } | { kind: "remembered" } | { kind: "paused" };

export type LimitAccountPresentation = {
  status: CardStatus | null;
  message: string | null;
  lastKnown: boolean;
  updatedAt: string | null;
};

export function headlineWindow(entry: ProviderLimits): { headline: LimitWindow | undefined; rest: LimitWindow[] } {
  const headline = entry.windows.find((window) => window.kind === "weekly");
  return { headline, rest: entry.windows.filter((window) => window !== headline) };
}

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

function elapsedNote(window: LimitWindow, now: number, resetAt: string): string {
  return `reset ${formatAgo(window.resetsAt, now)} · ${resetAt}`;
}

function pendingNote(resetIn: string, resetAt: string): string {
  return resetIn ? `resets in ${resetIn}${resetAt ? ` · ${resetAt}` : ""}` : "";
}

function mainWindows(entry: ProviderLimits): LimitWindow[] {
  return entry.windows.filter((window) => window.kind === "session" || window.kind === "weekly");
}

export function usageLeft(entry: ProviderLimits, now: number): number | null {
  const used = mainWindows(entry).map((window) => presentLimitWindow(window, now).percent);
  return used.length ? 100 - Math.max(...used) : null;
}

export function usableAgainAt(entry: ProviderLimits, now: number): number {
  return Math.max(
    ...mainWindows(entry)
      .filter((window) => presentLimitWindow(window, now).percent >= 100)
      .map((window) => parseInstant(window.resetsAt) ?? Infinity),
  );
}

export function unexpiredBankedResets(resetCredits: LimitsResetCredits | null | undefined, now: number): LimitsResetCredits | null {
  if (!resetCredits || resetCredits.availableCount <= 0) return null;
  return hasElapsed(resetCredits.nextExpiresAt, now) ? null : resetCredits;
}

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

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

export function presentCreditsSpent(spent: LimitsCreditsSpent): { value: string; note: string } {
  return {
    value: SPENT.format(spent.last7Days),
    note: `last 7 days · ${SPENT.format(spent.last30Days)} in 30 days`,
  };
}

export function hasObservations(entry: ProviderLimits): boolean {
  return (
    entry.windows.length > 0 ||
    entry.credits != null ||
    entry.workspaceCredits != null ||
    entry.creditsSpent != null ||
    (entry.resetCredits?.availableCount ?? 0) > 0
  );
}

export function latestObservedAt(entry: ProviderLimits): number | null {
  return entry.windows.reduce<number | null>((latest, window) => {
    const observedAt = parseInstant(window.observedAt);
    if (observedAt === null) return latest;
    return latest === null ? observedAt : Math.max(latest, observedAt);
  }, null);
}

export function presentLimitAccount(entry: ProviderLimits, fallbackMessage: string): LimitAccountPresentation {
  const observed = hasObservations(entry);
  const newest = latestObservedAt(entry);
  const savedProfile = entry.savedProfile === true;
  const savedRefreshPaused = savedProfile && observed && (entry.status === "failed" || entry.status === "unauthenticated");
  const detail = entry.message ?? fallbackMessage;
  return {
    status: savedRefreshPaused ? { kind: "savedRefresh", detail }
      : !entry.currentAccount && !savedProfile && observed ? { kind: "remembered" }
      : entry.status !== "ok" && observed ? { kind: "paused" }
      : null,
    message: entry.status === "ok" || savedRefreshPaused ? null : detail,
    lastKnown: entry.status !== "ok",
    updatedAt: newest === null ? null : formatObservedAt(new Date(newest).toISOString()),
  };
}
