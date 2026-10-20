import { formatObservedAt, hasElapsed, parseInstant } from "$lib/limitsFormat";
import type { LimitWindow, LimitsCreditsSpent, LimitsResetCredits, LimitsWorkspaceCredits, ProviderLimits, ReadStanding, StandingMessage } from "$lib/limitsTypes";
import { shareAt, windowAt, type WindowAt } from "$lib/windowAt";

export type Gauge = Pick<WindowAt, "usedNow" | "usedFigure" | "meter">;

export type LimitWindowPresentation = Gauge & { text: string; note: string };

export const CHECKING_LIMITS = "Checking limits…";

export type CardStatus = { kind: "savedRefresh"; detail: string } | { kind: "remembered" } | { kind: "paused" };

export type StandingPresentation = {
  status: CardStatus | null;
  message: StandingMessage | null;
  mayShowEmpty: boolean;
};

export function headlineWindow(entry: ProviderLimits): { headline: LimitWindow | undefined; rest: LimitWindow[] } {
  const headline = entry.windows.find((window) => window.kind === "weekly");
  return { headline, rest: entry.windows.filter((window) => window !== headline) };
}

function present(at: WindowAt, note: string): LimitWindowPresentation {
  return { usedNow: at.usedNow, usedFigure: at.usedFigure, meter: at.meter, text: at.usedText, note };
}

export function presentLimitWindow(window: LimitWindow, now: number): LimitWindowPresentation {
  const at = windowAt(window, now);
  return present(at, at.note.text);
}

function mainWindows(entry: ProviderLimits): LimitWindow[] {
  return entry.windows.filter((window) => window.kind === "session" || window.kind === "weekly");
}

export function usageLeft(entry: ProviderLimits, now: number): number | null {
  const used = mainWindows(entry).flatMap((window) => windowAt(window, now).usedNow ?? []);
  return used.length ? 100 - Math.max(...used) : null;
}

export function usageLeftFigure(entry: ProviderLimits, now: number): number | null {
  const figures = mainWindows(entry).flatMap((window) => windowAt(window, now).usedFigure ?? []);
  return figures.length ? 100 - Math.max(...figures) : null;
}

export function usableAgainAt(entry: ProviderLimits, now: number): number {
  return Math.max(
    ...mainWindows(entry)
      .filter((window) => windowAt(window, now).reached)
      .map((window) => parseInstant(window.resetsAt) ?? Infinity),
  );
}

export function unexpiredBankedResets(resetCredits: LimitsResetCredits | null | undefined, now: number): LimitsResetCredits | null {
  if (!resetCredits || resetCredits.availableCount <= 0) return null;
  return hasElapsed(resetCredits.nextExpiresAt, now) ? null : resetCredits;
}

const AMOUNT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

export function presentWorkspaceShare(share: LimitsWorkspaceCredits, now: number): LimitWindowPresentation {
  const at = shareAt(share, now);
  const limit = Number(share.limit);
  const used = Number(share.used);
  const amounts = at.note.kind === "renewed"
    ? `${AMOUNT.format(limit)} of ${AMOUNT.format(limit)} left`
    : !share.reached
      ? `${AMOUNT.format(Math.max(limit - used, 0))} of ${AMOUNT.format(limit)} left`
      : used >= limit
        ? `all ${AMOUNT.format(limit)} used`
        : "limit reached";
  return present(at, [amounts, at.note.text].filter(Boolean).join(" · "));
}

const SPENT = new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 });

export function presentCreditsSpent(spent: LimitsCreditsSpent): { value: string; note: string } {
  return {
    value: SPENT.format(spent.last7Days),
    note: `last 7 days · ${SPENT.format(spent.last30Days)} in 30 days`,
  };
}

export function latestObservedAt(entry: ProviderLimits): number | null {
  return entry.windows.reduce<number | null>((latest, window) => {
    const observedAt = parseInstant(window.observedAt);
    if (observedAt === null) return latest;
    return latest === null ? observedAt : Math.max(latest, observedAt);
  }, null);
}

export function observedAtText(entry: ProviderLimits): string | null {
  const newest = latestObservedAt(entry);
  return newest === null ? null : formatObservedAt(new Date(newest).toISOString());
}

const LIVE: StandingPresentation = { status: null, message: null, mayShowEmpty: true };

export function presentStanding(standing: ReadStanding | undefined): StandingPresentation {
  if (!standing) return LIVE;
  switch (standing.kind) {
    case "live": return LIVE;
    case "unavailable": return { status: null, message: standing.message, mayShowEmpty: false };
    case "lastObserved":
      switch (standing.why) {
        case "remembered": return { status: { kind: "remembered" }, message: null, mayShowEmpty: true };
        case "readPaused": return { status: { kind: "paused" }, message: standing.message, mayShowEmpty: false };
        case "savedRefreshPaused": return { status: { kind: "savedRefresh", detail: standing.message.text }, message: null, mayShowEmpty: false };
      }
  }
}
