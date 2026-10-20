import { describe, expect, it } from "vitest";
import type { AccountsReading, LoginState, SavedProfile } from "$lib/accountTypes";
import { untilSignInEnds } from "./accounts";

const NOW = Date.parse("2026-08-17T20:00:00Z");
const MINUTE = 60_000;
const at = (ms: number) => new Date(NOW + ms).toISOString();

function reading(...states: LoginState[]): AccountsReading {
  const profiles = states.map((loginState, index): SavedProfile => ({
    id: `saved-${index}`, observationId: `profile:${index}`, identity: { provider: "claude", userId: `${index}`, workspaceId: "team" },
    label: "you@example.com", email: "you@example.com", savedAt: at(0), active: index === 0, loginState,
  }));
  return { profiles, nativeAccount: null, recoveryRequired: false, notice: null };
}

describe("when the account list is read again for a sign-in's end", () => {
  it("is when the nearest ending sign-in ends, so the list says it ended", () => {
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(90 * MINUTE) }, { state: "ending", endsAt: at(30 * MINUTE) }), NOW)).toBe(30 * MINUTE);
  });

  it("is never sooner than a minute, nor later than a timer can wait", () => {
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(5_000) }), NOW)).toBe(MINUTE);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(-5_000) }), NOW)).toBe(MINUTE);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(90 * 24 * 60 * MINUTE) }), NOW)).toBe(2 ** 31 - 1);
  });

  it("is never, when no sign-in is ending", () => {
    expect(untilSignInEnds(undefined, NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "usable" }, { state: "ended", endsAt: at(-MINUTE) }, { state: "refused" }), NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: "not a date" }), NOW)).toBe(false);
  });
});
