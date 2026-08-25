import {
  formatAgo,
  formatObservedAt,
  formatResetAt,
  formatResetIn,
  formatUsedPercent,
  hasElapsed,
  usageTone,
  type UsageTone,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";

/** The meter's spoken value once a reset has passed: the empty track is honest about why. */
export const RESET_VALUE_TEXT = "not observed since the reset";

export type LimitWindowPresentation = {
  percent: number;
  tone: UsageTone;
  text: string;
  note: string;
  unavailable: boolean;
};

export type LimitAccountPresentation = {
  message: string | null;
  refreshPaused: boolean;
  remembered: boolean;
  updatedAt: string | null;
};

/**
 * Present one independently observed quota window. A value remains usable only until its own
 * known reset passes. Account status cannot make an observation from the prior cycle current.
 *
 * Once the reset has passed, the window is presented as what it is — reset, unobserved since —
 * rather than as a missing value: when the reset happened, and what the window held before it,
 * so a remembered account still tells the user its quota has renewed.
 */
export function presentLimitWindow(window: LimitWindow, now: number): LimitWindowPresentation {
  const unavailable = hasElapsed(window.resetsAt, now);
  const percent = unavailable ? 0 : window.usedPercent;
  const tone = usageTone(percent);
  const resetAt = formatResetAt(window.resetsAt);
  const note = unavailable
    ? [`reset ${formatAgo(window.resetsAt, now)}`, resetAt, `was ${formatUsedPercent(window.usedPercent)}`]
        .filter(Boolean)
        .join(" · ")
    : resetInNote(formatResetIn(window.resetsAt, now), resetAt);

  return {
    percent,
    tone,
    text: unavailable ? "—" : formatUsedPercent(percent),
    note,
    unavailable,
  };
}

function resetInNote(resetIn: string, resetAt: string): string {
  return resetIn ? `resets in ${resetIn}${resetAt ? ` · ${resetAt}` : ""}` : "";
}

export function presentLimitAccount(entry: ProviderLimits, fallbackMessage: string): LimitAccountPresentation {
  const hasObservations = entry.windows.length > 0 || entry.credits != null;
  const latestObservedAt = entry.windows.reduce<number | null>((latest, window) => {
    const observedAt = Date.parse(window.observedAt);
    if (Number.isNaN(observedAt)) return latest;
    return latest === null ? observedAt : Math.max(latest, observedAt);
  }, null);
  return {
    message: entry.status === "ok" ? null : (entry.message ?? fallbackMessage),
    refreshPaused: entry.currentAccount && entry.status !== "ok" && hasObservations,
    remembered: !entry.currentAccount && hasObservations,
    updatedAt: latestObservedAt === null ? null : formatObservedAt(new Date(latestObservedAt).toISOString()),
  };
}
