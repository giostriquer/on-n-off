import {
  formatObservedAt,
  formatResetAt,
  formatResetIn,
  formatUsedPercent,
  hasElapsed,
  usageTone,
  type UsageTone,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";

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
};

/**
 * Present one independently observed quota window. A value remains usable only until its own
 * known reset passes. Account status cannot make an observation from the prior cycle current.
 */
export function presentLimitWindow(window: LimitWindow, now: number): LimitWindowPresentation {
  const unavailable = hasElapsed(window.resetsAt, now);
  const percent = unavailable ? 0 : window.usedPercent;
  const tone = usageTone(percent);
  const resetAt = formatResetAt(window.resetsAt);
  const resetIn = formatResetIn(window.resetsAt, now);
  const updatedAt = formatObservedAt(window.observedAt);
  const updatedNote = `last updated ${updatedAt || "at an unknown time"}`;
  const resetNote = resetIn ? `resets in ${resetIn}${resetAt ? ` · ${resetAt}` : ""}` : "";
  const note = unavailable ? `Current usage unknown · ${updatedNote}` : [resetNote, updatedNote].filter(Boolean).join(" · ");

  return {
    percent,
    tone,
    text: unavailable ? "—" : formatUsedPercent(percent),
    note,
    unavailable,
  };
}

export function presentLimitAccount(entry: ProviderLimits, fallbackMessage: string): LimitAccountPresentation {
  const hasObservations = entry.windows.length > 0 || entry.credits != null;
  return {
    message: entry.status === "ok" ? null : (entry.message ?? fallbackMessage),
    refreshPaused: entry.currentAccount && entry.status !== "ok" && hasObservations,
    remembered: !entry.currentAccount && hasObservations,
  };
}
