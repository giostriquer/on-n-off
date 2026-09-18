/** Display formatting for the Limits screen (pure). */

import { providerColor } from "./providerStyle";
import type { AgentId } from "./types";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

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

/**
 * "Aug 29" in the given or the viewer's time zone, for instants weeks away where a weekday alone
 * would be ambiguous; empty when unknown. `withYear` always adds the year; `yearUnlessSameAs` adds
 * it only when the date falls in another year than that instant, both judged in the same zone.
 */
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

/** Where a meter starts hardening, and where it is spent. Both twins use the same two numbers. */
const HARDENS_FROM = 70;
const SPENT = 90;

/**
 * The app's one usage ramp, shared by every surface that shows a quota filling up.
 *
 * A meter stays on its base colour while there is room, then hardens toward `--trip` as it fills,
 * reaching it at `SPENT`. It never passes through `--warn`: that amber's hue points away from red,
 * and on the dark theme it is lighter than the accents it would replace, so a meter stepping into
 * it went paler and yellower exactly as it ran out, which reads as cooling down. `--warn` still
 * means "pending" on CI rollups, badges and hints, where nothing is filling up.
 *
 * The blend is eased rather than linear — a quarter of the way through the band is half the way to
 * red — so crossing 70 % announces itself instead of creeping. `color-mix` is used rather than
 * numeric interpolation so the ramp follows whichever theme is active.
 *
 * The side notch runs the same ramp over the same endpoints, in Swift (`NotchCore/Meter.swift`) and
 * in Rust (`side_notch/model.rs`). Change the shape here and change it in both of those.
 */
export function usageMeterColor(base: string, usedPercent: number): string {
  if (usedPercent >= SPENT) return "var(--trip)";
  // Negated rather than `<= HARDENS_FROM`, so a non-numeric percent lands on the base colour like
  // it does in both twins, instead of falling through and emitting `NaN%` — invalid CSS, which the
  // browser drops, leaving an empty bar in the very band that matters.
  if (!(usedPercent > HARDENS_FROM)) return base;
  const travelled = Math.sqrt((usedPercent - HARDENS_FROM) / (SPENT - HARDENS_FROM));
  return `color-mix(in srgb, ${base}, var(--trip) ${(travelled * 100).toFixed(1)}%)`;
}

/** A quota bar: the provider's accent, hardening toward red as the window fills. */
export function usageFillColor(provider: AgentId, usedPercent: number): string {
  return usageMeterColor(providerColor(provider), usedPercent);
}

/**
 * The same fill as a style object, with the flat accent underneath it.
 *
 * If a webview ever cannot parse the `color-mix`, it drops that declaration and the accent stands.
 * Without the pair the bar would render with no background at all — an empty track, which reads as
 * unused, in exactly the band where the meter is trying to warn.
 */
export function usageFillStyle(provider: AgentId, usedPercent: number) {
  return { background: providerColor(provider), backgroundColor: usageFillColor(provider, usedPercent) };
}

/**
 * The percent figure beside the bar: ordinary text until the window is spent, then `--trip`.
 *
 * It deliberately does not follow the bar through the band. The figure sits on the page's own ink,
 * and blending white toward red gives a washed-out pink that is *less* legible at 75 % than the
 * plain figure was at 50 % — the paling problem again, one surface over. The notch does the same
 * thing: its ring carries the ramp while its label stays plain.
 */
export function usageTextColor(usedPercent: number): string | undefined {
  return usedPercent >= SPENT ? "var(--trip)" : undefined;
}

export function formatUsedPercent(usedPercent: number): string {
  if (usedPercent > 0 && usedPercent < 1) return "<1%";
  return `${Math.round(usedPercent)}%`;
}

/** "max" → "Max", "enterprise_x" → "Enterprise x"; empty when unknown. */
export function planLabel(plan: string | null | undefined, provider?: string): string {
  const raw = plan?.trim().replaceAll("_", " ") ?? "";
  if (!raw) return "";
  if (provider === "codex") {
    const code = raw.toLowerCase().replaceAll(/[ _-]/g, "");
    if (code === "pro") return "Pro ×20";
    if (code === "prolite") return "Pro ×5";
  }
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}

/** Symbols for the currencies this app is likely to meet; anything else prints its ISO code. */
const CURRENCY_SYMBOLS: Record<string, string> = { USD: "$", EUR: "€", GBP: "£", JPY: "¥" };

/**
 * A provider's price for a paid reset, in the currency's minor units (cents). Returns null when
 * the provider named no amount, which is an offer without a price rather than a free one.
 */
export function formatOfferPrice(offer: { currency?: string | null; amountMinorUnits?: number | null }): string | null {
  const minor = offer.amountMinorUnits;
  if (typeof minor !== "number" || !Number.isFinite(minor) || minor < 0) return null;
  const code = offer.currency?.trim().toUpperCase();
  // JPY and its kind have no minor unit; every other currency here carries two digits.
  const amount = code === "JPY" ? String(Math.round(minor)) : (minor / 100).toFixed(2);
  if (!code) return amount;
  const symbol = CURRENCY_SYMBOLS[code];
  return symbol ? `${symbol}${amount}` : `${code} ${amount}`;
}
