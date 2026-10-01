import { providerColor } from "./providerStyle";
import type { AgentId } from "./types";

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

const HARDENS_FROM = 70;
const SPENT = 90;

export function usageMeterColor(base: string, usedPercent: number): string {
  if (usedPercent >= SPENT) return "var(--trip)";
  if (!(usedPercent > HARDENS_FROM)) return base;
  const travelled = Math.sqrt((usedPercent - HARDENS_FROM) / (SPENT - HARDENS_FROM));
  return `color-mix(in srgb, ${base}, var(--trip) ${(travelled * 100).toFixed(1)}%)`;
}

export function usageFillColor(provider: AgentId, usedPercent: number): string {
  return usageMeterColor(providerColor(provider), usedPercent);
}

export function usageFillStyle(provider: AgentId, usedPercent: number) {
  return { background: providerColor(provider), backgroundColor: usageFillColor(provider, usedPercent) };
}

export function usageTextColor(usedPercent: number): string | undefined {
  return usedPercent >= SPENT ? "var(--trip)" : undefined;
}

export function formatUsedPercent(usedPercent: number): string {
  if (usedPercent > 0 && usedPercent < 1) return "<1%";
  return `${Math.round(usedPercent)}%`;
}

const CODEX_PRO_TIERS: Record<string, number> = { pro: 20, prolite: 5 };

function codexProTier(plan: string, provider?: string): number | undefined {
  if (provider !== "codex") return undefined;
  return CODEX_PRO_TIERS[plan.toLowerCase().replaceAll(/[ _-]/g, "")];
}

export function planLabel(plan: string | null | undefined, provider?: string): string {
  const raw = plan?.trim().replaceAll("_", " ") ?? "";
  if (!raw) return "";
  const proTier = codexProTier(raw, provider);
  if (proTier) return `Pro ×${proTier}`;
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}

export function planMultiplier(plan: string | null | undefined, provider?: string): number {
  const raw = plan?.trim() ?? "";
  const proTier = codexProTier(raw, provider);
  if (proTier) return proTier;
  const multiplier = /[×x]\s*(\d+)/i.exec(raw)?.[1];
  if (multiplier) return Number(multiplier);
  return raw.toLowerCase() === "max" ? 5 : 1;
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
