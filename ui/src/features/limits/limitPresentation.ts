import {
  formatObservedAt,
  formatResetAt,
  formatResetIn,
  formatUsedPercent,
  hasElapsed,
  usageTone,
  usageToneColor,
  type UsageTone,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { formatAgo } from "$lib/timeFormat";

export type LimitWindowPresentation = {
  percent: number;
  tone: UsageTone;
  /** The value slot: the window's percentage, which a passed reset puts back at zero. */
  text: string;
  /** Colour for the value slot; undefined leaves the default ink. */
  color: string | undefined;
  note: string;
};

export type LimitAccountPresentation = {
  message: string | null;
  refreshPaused: boolean;
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
  const tone = usageTone(usedPercent);
  return {
    percent: usedPercent,
    tone,
    text: formatUsedPercent(usedPercent),
    color: usageToneColor(tone),
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

export function presentLimitAccount(entry: ProviderLimits, fallbackMessage: string): LimitAccountPresentation {
  const windows = visibleLimitWindows(entry);
  const hasObservations = windows.length > 0 || entry.credits != null;
  const latestObservedAt = windows.reduce<number | null>((latest, window) => {
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
