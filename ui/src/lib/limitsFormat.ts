import { providerColor } from "./providers";
import type { AgentId } from "./types";
import type { MeterLevel } from "./windowAt";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

export function parseInstant(value: string | null | undefined): number | null {
  if (!value) return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : ms;
}

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

export function hasElapsed(iso: string | null | undefined, nowMs: number): boolean {
  const at = parseInstant(iso);
  return at !== null && at <= nowMs;
}

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

export function formatShortDate(
  iso: string | null | undefined,
  { timeZone, withYear = false, yearUnlessSameAs }: { timeZone?: string; withYear?: boolean; yearUnlessSameAs?: number } = {},
): string {
  const at = parseInstant(iso);
  if (at === null) return "";
  const yearOf = (ms: number) => new Intl.DateTimeFormat("en-US", { year: "numeric", timeZone }).format(ms);
  const year = withYear || (yearUnlessSameAs !== undefined && yearOf(at) !== yearOf(yearUnlessSameAs));
  return new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: year ? "numeric" : undefined, timeZone }).format(at);
}

export function formatExactDate(iso: string | null | undefined): string {
  const at = parseInstant(iso);
  if (at === null) return "";
  return new Date(at).toLocaleString(undefined, {
    year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

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

export function usageMeterColor(base: string, meter: MeterLevel): string {
  switch (meter.level) {
    case "trip": return "var(--trip)";
    case "blend": return `color-mix(in srgb, ${base}, var(--trip) ${(meter.fraction * 100).toFixed(1)}%)`;
    case "base":
    case "unreadable": return base;
  }
}

export function usageFillStyle(provider: AgentId, meter: MeterLevel) {
  return { background: providerColor(provider), backgroundColor: usageMeterColor(providerColor(provider), meter) };
}

export function usageTextColor(meter: MeterLevel): string | undefined {
  return meter.level === "trip" ? "var(--trip)" : undefined;
}

export function formatPrice({ amountMinorUnits, currency }: { amountMinorUnits: number; currency: string }): string {
  try {
    const format = new Intl.NumberFormat("en-US", { style: "currency", currency });
    const digits = format.resolvedOptions().maximumFractionDigits ?? 2;
    return format.format(amountMinorUnits / 10 ** digits).replace(/\u00a0/g, " ");
  } catch {
    return `${currency} ${amountMinorUnits / 100}`;
  }
}
