import { formatResetAt, formatResetIn, formatShortDate, hasElapsed, parseInstant } from "./limitsFormat";
import type { LimitWindow, LimitsWorkspaceCredits } from "./limitsTypes";
import { providerColor } from "./providers";
import { formatAgo } from "./timeFormat";
import type { AgentId } from "./types";

export type MeterLevel = { level: "unreadable" } | { level: "base" } | { level: "blend"; fraction: number } | { level: "trip" };

export type ResetNote =
  | { kind: "pending"; when: string; relative?: string; text: string }
  | { kind: "renewed"; when: string; relative: string; text: string }
  | { kind: "idle"; text: string }
  | { kind: "unknown"; text: string };

export type WindowAt = {
  usedNow: number | null;
  usedFigure: number | null;
  reached: boolean;
  usedText: string;
  meter: MeterLevel;
  note: ResetNote;
};

const REACHED = 100;
const LAST_UNREACHED = 99;
const SLIVER = 1;
const HARDENS_FROM = 70;
const TRIPPED = 90;

function readable(usedPercent: number): number | null {
  return Number.isFinite(usedPercent) && usedPercent >= 0 && usedPercent <= REACHED ? usedPercent : null;
}

function isSliver(used: number): boolean {
  return used > 0 && used < 1;
}

function figureOf(used: number): number {
  if (used >= REACHED) return REACHED;
  if (isSliver(used)) return SLIVER;
  return Math.min(Math.round(used), LAST_UNREACHED);
}

function meterLevel(usedNow: number | null): MeterLevel {
  if (usedNow === null) return { level: "unreadable" };
  if (usedNow >= TRIPPED) return { level: "trip" };
  if (!(usedNow > HARDENS_FROM)) return { level: "base" };
  return { level: "blend", fraction: Math.sqrt((usedNow - HARDENS_FROM) / (TRIPPED - HARDENS_FROM)) };
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

type Subject = {
  spell: (reset: string, timeZone?: string) => string;
  countdown: boolean;
  unknown: string;
};

const WINDOW: Subject = { spell: (reset, timeZone) => formatResetAt(reset, timeZone), countdown: true, unknown: "Reset time unavailable" };

const SHARE: Subject = { spell: (reset, timeZone) => formatShortDate(reset, { timeZone }), countdown: false, unknown: "" };

function resetNote(subject: Subject, reset: string | null, startsOnFirstMessage: boolean, now: number, timeZone?: string): ResetNote {
  if (reset === null) return startsOnFirstMessage ? { kind: "idle", text: "Starts with your first message" } : { kind: "unknown", text: subject.unknown };
  const when = subject.spell(reset, timeZone);
  if (hasElapsed(reset, now)) {
    const relative = formatAgo(reset, now);
    return { kind: "renewed", when, relative, text: `reset ${relative} · ${when}` };
  }
  if (!subject.countdown) return { kind: "pending", when, text: `resets ${when}` };
  const relative = formatResetIn(reset, now);
  return { kind: "pending", when, relative, text: `resets in ${relative} · ${when}` };
}

function readWindow(subject: Subject, usedPercent: number, resetsAt: string | null | undefined, startsOnFirstMessage: boolean, now: number, timeZone?: string): WindowAt {
  const reset = resetsAt != null && parseInstant(resetsAt) !== null ? resetsAt : null;
  const note = resetNote(subject, reset, startsOnFirstMessage, now, timeZone);
  const read = readable(usedPercent);
  const usedNow = read === null ? null : note.kind === "renewed" ? 0 : read;
  return {
    usedNow,
    usedFigure: usedNow === null ? null : figureOf(usedNow),
    reached: usedNow !== null && usedNow >= REACHED,
    usedText: usedNow === null ? "—" : isSliver(usedNow) ? "<1%" : `${figureOf(usedNow)}%`,
    meter: meterLevel(usedNow),
    note,
  };
}

export function windowAt(window: LimitWindow, now: number, timeZone?: string): WindowAt {
  return readWindow(WINDOW, window.usedPercent, window.resetsAt, window.startsOnFirstMessage === true, now, timeZone);
}

export function shareAt(share: LimitsWorkspaceCredits, now: number, timeZone?: string): WindowAt {
  return readWindow(SHARE, share.usedPercent, share.resetsAt, false, now, timeZone);
}
