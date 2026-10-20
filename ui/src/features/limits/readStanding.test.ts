import { expect, it } from "vitest";
import type { LimitsColumnEntry, ReadStanding, StandingMessage } from "$lib/limitsTypes";
import { hasLimits, isAgentId } from "$lib/providers";
import standings from "../../../../src-tauri/src/limits/standing/fixtures/standing-cases.json";
import { limitCards } from "./limitCards";
import { CHECKING_LIMITS } from "./limitPresentation";

const NOW = Date.parse("2026-09-01T12:00:00Z");

function exactly(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error(`${JSON.stringify(value)} is not an object`);
  const found = Object.keys(value).sort();
  if (found.join() !== [...keys].sort().join()) throw new Error(`${JSON.stringify(value)} does not hold exactly ${keys.join(", ")}`);
  return value as Record<string, unknown>;
}

function standingMessage(value: unknown): StandingMessage {
  const { text, tone } = exactly(value, ["text", "tone"]);
  if (typeof text !== "string" || (tone !== "error" && tone !== "muted")) throw new Error(`${JSON.stringify(value)} is not a message`);
  return { text, tone };
}

function readStanding(value: unknown): ReadStanding {
  const { kind, why } = value as { kind?: unknown; why?: unknown };
  if (kind === "live") {
    exactly(value, ["kind"]);
    return { kind };
  }
  if (kind === "unavailable") return { kind, message: standingMessage(exactly(value, ["kind", "message"]).message) };
  if (kind === "lastObserved" && why === "remembered") {
    exactly(value, ["kind", "why"]);
    return { kind, why };
  }
  if (kind === "lastObserved" && (why === "readPaused" || why === "savedRefreshPaused")) {
    return { kind, why, message: standingMessage(exactly(value, ["kind", "why", "message"]).message) };
  }
  throw new Error(`${JSON.stringify(value)} is not a read standing the UI names`);
}

it.each(standings.cases)("draws $name as its read standing says, whatever the read's status", ({ provider, standing, card: shown }) => {
  if (!isAgentId(provider) || !hasLimits(provider)) throw new Error(`no Limits card for ${provider}`);
  const entry: LimitsColumnEntry = { provider, status: "ok", currentAccount: true, windows: [], standing: readStanding(standing) };
  const [card] = limitCards({ provider, entries: [entry], nativeLogin: null, now: NOW }) ?? [];
  expect({ status: card.status, message: card.freshness.message }).toEqual(shown);
});

it("says it is checking in the words the notch uses", () => {
  expect(CHECKING_LIMITS).toBe(standings.checking);
});
