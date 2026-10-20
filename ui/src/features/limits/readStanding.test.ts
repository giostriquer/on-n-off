import { expect, it } from "vitest";
import type { ProviderLimits, ReadStanding } from "$lib/limitsTypes";
import { hasLimits, isAgentId } from "$lib/providers";
import standings from "../../../../src-tauri/src/limits/standing/fixtures/standing-cases.json";
import { limitCards } from "./limitCards";
import { CHECKING_LIMITS } from "./limitPresentation";

const NOW = Date.parse("2026-09-01T12:00:00Z");

it.each(standings.cases)("draws $name as its read standing says, whatever the read's status", ({ provider, standing, card: shown }) => {
  if (!isAgentId(provider) || !hasLimits(provider)) throw new Error(`no Limits card for ${provider}`);
  const reading: ProviderLimits = { provider, status: "ok", currentAccount: true, windows: [], standing: standing as ReadStanding };
  const [card] = limitCards({ provider, entries: [reading], profiles: [], nativeLogin: null, now: NOW }) ?? [];
  expect({ status: card.status, message: card.freshness.message }).toEqual(shown);
});

it("says it is checking in the words the notch uses", () => {
  expect(CHECKING_LIMITS).toBe(standings.checking);
});
