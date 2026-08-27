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

/** The meter's spoken value once a reset has passed: the empty track is honest about why. */
const RESET_VALUE_TEXT = "not observed since the reset";

export type LimitWindowPresentation = {
  percent: number;
  tone: UsageTone;
  /** The value slot: the percentage, or a dash once the window's reset has passed. */
  text: string;
  /** Colour for the value slot; undefined leaves the default ink. */
  color: string | undefined;
  note: string;
  /** What the meter announces instead of its (empty) percentage; undefined while the bar is true. */
  valueText: string | undefined;
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
 * Present one independently observed quota window. A value remains usable only until its own
 * known reset passes. Account status cannot make an observation from the prior cycle current.
 *
 * Once the reset has passed, the window is presented as what it is — reset, unobserved since —
 * rather than as a missing value: when the reset happened, and what the window held when it was
 * last seen, so a remembered account still tells the user its quota has renewed.
 */
export function presentLimitWindow(window: LimitWindow, now: number): LimitWindowPresentation {
  const resetAt = formatResetAt(window.resetsAt);
  if (hasElapsed(window.resetsAt, now)) {
    return {
      percent: 0,
      tone: "calm",
      text: "—",
      color: "var(--mute)",
      note: elapsedNote(window, now, resetAt),
      valueText: RESET_VALUE_TEXT,
    };
  }
  const tone = usageTone(window.usedPercent);
  return {
    percent: window.usedPercent,
    tone,
    text: formatUsedPercent(window.usedPercent),
    color: usageToneColor(tone),
    note: pendingNote(formatResetIn(window.resetsAt, now), resetAt),
    valueText: undefined,
  };
}

/** "reset 1h ago · Mon 20:34 · last seen 97%": the number dates from the observation, not the reset. */
function elapsedNote(window: LimitWindow, now: number, resetAt: string): string {
  return `reset ${formatAgo(window.resetsAt, now)} · ${resetAt} · last seen ${formatUsedPercent(window.usedPercent)}`;
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
