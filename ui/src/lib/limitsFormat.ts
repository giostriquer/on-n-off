/** Display formatting for the Limits screen (pure). */

import { providerColor } from "./providerStyle";
import type { AgentId } from "./types";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

export type UsageTone = "calm" | "warn" | "trip";

function parseInstant(value: string | null | undefined): number | null {
  if (!value) return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : ms;
}

/**
 * "2d 2h" / "4h 5m" / "12m" / "<1m" until the window resets, phrased to follow "resets in";
 * empty when the instant is unknown or already past (see `hasElapsed`).
 */
export function formatResetIn(resetsAt: string | null | undefined, nowMs: number): string {
  const at = parseInstant(resetsAt);
  if (at === null) return "";
  const remaining = at - nowMs;
  if (remaining <= 0) return "";
  if (remaining < MINUTE_MS) return "<1m";
  if (remaining >= DAY_MS) {
    const days = Math.floor(remaining / DAY_MS);
    const hours = Math.floor((remaining % DAY_MS) / HOUR_MS);
    return `${days}d ${hours}h`;
  }
  if (remaining >= HOUR_MS) {
    const hours = Math.floor(remaining / HOUR_MS);
    const minutes = Math.floor((remaining % HOUR_MS) / MINUTE_MS);
    return `${hours}h ${minutes}m`;
  }
  return `${Math.floor(remaining / MINUTE_MS)}m`;
}

/** True when `iso` is a known instant at or before `nowMs` (a window whose reset has passed). */
export function hasElapsed(iso: string | null | undefined, nowMs: number): boolean {
  const at = parseInstant(iso);
  return at !== null && at <= nowMs;
}

/** "Tue 14:00" in the given (or the viewer's) time zone; empty when unknown. */
export function formatResetAt(resetsAt: string | null | undefined, timeZone?: string): string {
  const at = parseInstant(resetsAt);
  if (at === null) return "";
  return new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  }).format(at);
}

/** "Aug 19, 2026, 02:04" in the given (or viewer's) time zone; empty when unknown. */
export function formatObservedAt(observedAt: string | null | undefined, timeZone?: string): string {
  const at = parseInstant(observedAt);
  if (at === null) return "";
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  }).format(at);
}

/** "14:05" (24h) in the given (or the viewer's) time zone; empty when unknown. */
export function formatClock(iso: string | null | undefined, timeZone?: string): string {
  const at = parseInstant(iso);
  if (at === null) return "";
  return new Intl.DateTimeFormat("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  }).format(at);
}

export function usageTone(usedPercent: number): UsageTone {
  if (usedPercent >= 90) return "trip";
  if (usedPercent >= 70) return "warn";
  return "calm";
}

export function usageToneColor(tone: UsageTone): string | undefined {
  if (tone === "warn") return "var(--warn)";
  if (tone === "trip") return "var(--trip)";
  return undefined;
}

export function usageFillColor(provider: AgentId, tone: UsageTone): string {
  return usageToneColor(tone) ?? providerColor(provider);
}

export function formatUsedPercent(usedPercent: number): string {
  if (usedPercent > 0 && usedPercent < 1) return "<1%";
  return `${Math.round(usedPercent)}%`;
}

/** "max" → "Max", "enterprise_x" → "Enterprise x"; empty when unknown. */
export function planLabel(plan: string | null | undefined): string {
  const raw = plan?.trim().replaceAll("_", " ") ?? "";
  if (!raw) return "";
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}
