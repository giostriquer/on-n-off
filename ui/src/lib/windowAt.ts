import { formatResetAt, formatResetIn, formatShortDate, hasElapsed, parseInstant } from "./limitsFormat";
import type { LimitWindow, LimitsWorkspaceCredits } from "./limitsTypes";
import { formatAgo } from "./timeFormat";

export type MeterLevel = { level: "unreadable" } | { level: "base" } | { level: "blend"; fraction: number } | { level: "trip" };

export type ResetNote = { kind: "pending" | "elapsed" | "idle" | "unknown"; at?: string; relative?: string; text: string };

export type WindowAt = {
  usedNow: number | null;
  renewed: boolean;
  reached: boolean;
  idle: boolean;
  usedText: string;
  meter: MeterLevel;
  note: ResetNote;
};

const REACHED = 100;
const LAST_UNREACHED = 99;
const HARDENS_FROM = 70;
const TRIPPED = 90;

function readable(usedPercent: number): number | null {
  return Number.isFinite(usedPercent) && usedPercent >= 0 && usedPercent <= REACHED ? usedPercent : null;
}

function isReached(usedNow: number | null): boolean {
  return usedNow !== null && usedNow >= REACHED;
}

function usedText(usedNow: number | null): string {
  if (usedNow === null) return "—";
  if (isReached(usedNow)) return "100%";
  if (usedNow > 0 && usedNow < 1) return "<1%";
  return `${Math.min(Math.round(usedNow), LAST_UNREACHED)}%`;
}

export function meterLevel(usedNow: number | null): MeterLevel {
  if (usedNow === null) return { level: "unreadable" };
  if (usedNow >= TRIPPED) return { level: "trip" };
  if (!(usedNow > HARDENS_FROM)) return { level: "base" };
  return { level: "blend", fraction: Math.sqrt((usedNow - HARDENS_FROM) / (TRIPPED - HARDENS_FROM)) };
}

type Subject = {
  at: (resetsAt: string, timeZone?: string) => string;
  pending: (relative: string, at: string) => { relative?: string; text: string };
  unknown: string;
};

const WINDOW: Subject = {
  at: (resetsAt, timeZone) => formatResetAt(resetsAt, timeZone),
  pending: (relative, at) => ({ relative, text: `resets in ${relative} · ${at}` }),
  unknown: "Reset time unavailable",
};

const SHARE: Subject = {
  at: (resetsAt, timeZone) => formatShortDate(resetsAt, { timeZone }),
  pending: (_, at) => ({ text: `resets ${at}` }),
  unknown: "",
};

function resetNote(subject: Subject, reset: string | null, renewed: boolean, idle: boolean, now: number, timeZone?: string): ResetNote {
  if (reset === null) return idle ? { kind: "idle", text: "Starts with your first message" } : { kind: "unknown", text: subject.unknown };
  const at = subject.at(reset, timeZone);
  if (!renewed) return { kind: "pending", at, ...subject.pending(formatResetIn(reset, now), at) };
  const relative = formatAgo(reset, now);
  return { kind: "elapsed", at, relative, text: `reset ${relative} · ${at}` };
}

function at(subject: Subject, usedPercent: number, resetsAt: string | null | undefined, startsOnFirstMessage: boolean, now: number, timeZone?: string): WindowAt {
  const reset = resetsAt != null && parseInstant(resetsAt) !== null ? resetsAt : null;
  const renewed = hasElapsed(reset, now);
  const read = readable(usedPercent);
  const usedNow = read === null ? null : renewed ? 0 : read;
  const idle = startsOnFirstMessage && reset === null && usedNow === 0;
  return {
    usedNow,
    renewed,
    reached: isReached(usedNow),
    idle,
    usedText: usedText(usedNow),
    meter: meterLevel(usedNow),
    note: resetNote(subject, reset, renewed, idle, now, timeZone),
  };
}

export function windowAt(window: LimitWindow, now: number, timeZone?: string): WindowAt {
  return at(WINDOW, window.usedPercent, window.resetsAt, window.startsOnFirstMessage === true, now, timeZone);
}

export function shareAt(share: LimitsWorkspaceCredits, now: number, timeZone?: string): WindowAt {
  return at(SHARE, share.usedPercent, share.resetsAt, false, now, timeZone);
}
