import { describe, expect, it } from "vitest";
import type { AccountsReading, LoginState, SavedProfile } from "$lib/accountTypes";
import { accountListQuery, untilSignInEnds } from "./accounts";

const NOW = Date.parse("2026-08-17T20:00:00Z");
const MINUTE = 60_000;
const JUST_AFTER = 1_000;
const at = (ms: number) => new Date(NOW + ms).toISOString();

function reading(...states: LoginState[]): AccountsReading {
  const profiles = states.map((loginState, index): SavedProfile => ({
    id: `saved-${index}`, observationId: `profile:${index}`, identity: { provider: "claude", userId: `${index}`, workspaceId: "team" },
    label: "you@example.com", email: "you@example.com", savedAt: at(0), active: index === 0, loginState,
  }));
  return { profiles, nativeAccount: null, recoveryRequired: false, notice: null };
}

describe("when the account list is read again for a sign-in's end", () => {
  it("is just after the nearest sign-in still to end, so the list says it ended", () => {
    const later = { state: "ending", endsAt: at(90 * MINUTE) } as const;
    expect(untilSignInEnds(reading(later, { state: "ending", endsAt: at(30 * MINUTE) }), NOW)).toBe(30 * MINUTE + JUST_AFTER);
    expect(untilSignInEnds(reading(later, { state: "ending", endsAt: at(-5_000) }), NOW)).toBe(90 * MINUTE + JUST_AFTER);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(5_000) }), NOW)).toBe(5_000 + JUST_AFTER);
  });

  it("is never later than a timer can wait", () => {
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(90 * 24 * 60 * MINUTE) }), NOW)).toBe(2 ** 31 - 1);
  });

  it("is never for a date that has already passed, so a list that still says ending is not asked again and again", () => {
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(-5_000) }), NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(0) }), NOW)).toBe(false);
  });

  it("is never, when no sign-in is ending", () => {
    expect(untilSignInEnds(undefined, NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "usable" }, { state: "ended", endsAt: at(-MINUTE) }, { state: "refused" }), NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: "not a date" }), NOW)).toBe(false);
  });
});

describe("the account list's own refetch", () => {
  it("waits for the next ending date only while the list reads, never while it fails", () => {
    const refetchInterval = accountListQuery("claude").refetchInterval as (query: unknown) => number | false;
    const data = reading({ state: "ending", endsAt: new Date(Date.now() + 30 * MINUTE).toISOString() });
    expect(refetchInterval({ state: { status: "success", data } })).toBeGreaterThan(29 * MINUTE);
    expect(refetchInterval({ state: { status: "error", data } })).toBe(false);
  });
});
