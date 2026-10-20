import { describe, expect, it } from "vitest";
import type { AccountsReading, LoginState, SavedProfile } from "$lib/accountTypes";
import { accountListQuery, signInEndedSinceRead, untilSignInEnds } from "./accounts";

const NOW = Date.parse("2026-08-17T20:00:00Z");
const MINUTE = 60_000;
const JUST_AFTER = 1_000;
const at = (ms: number) => new Date(NOW + ms).toISOString();
const READ_EARLIER = NOW - 10 * MINUTE;

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
    expect(untilSignInEnds(reading(later, { state: "ending", endsAt: at(30 * MINUTE) }), NOW, NOW)).toBe(30 * MINUTE + JUST_AFTER);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(5_000) }), NOW, NOW)).toBe(5_000 + JUST_AFTER);
  });

  it("is never later than a timer can wait", () => {
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(90 * 24 * 60 * MINUTE) }), NOW, NOW)).toBe(2 ** 31 - 1);
  });

  it("is every minute once a date has passed since the list was read, until a read after it lands", () => {
    const passed = reading({ state: "ending", endsAt: at(-5_000) }, { state: "ending", endsAt: at(90 * MINUTE) });
    expect(untilSignInEnds(passed, READ_EARLIER, NOW)).toBe(MINUTE);
    expect(signInEndedSinceRead(passed, READ_EARLIER, NOW)).toBe(true);
    expect(untilSignInEnds(passed, NOW, NOW), "read after the date and still ending").toBe(90 * MINUTE + JUST_AFTER);
    expect(signInEndedSinceRead(passed, NOW, NOW)).toBe(false);
  });

  it("is never for a date the list was already read after, so a list that still says ending is not asked again and again", () => {
    const passed = reading({ state: "ending", endsAt: at(-5_000) });
    expect(untilSignInEnds(passed, NOW, NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: at(0) }), NOW, NOW)).toBe(false);
    expect(signInEndedSinceRead(passed, NOW, NOW)).toBe(false);
  });

  it("is never, when no sign-in is ending", () => {
    expect(untilSignInEnds(undefined, READ_EARLIER, NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "usable" }, { state: "ended", endsAt: at(-MINUTE) }, { state: "refused" }), READ_EARLIER, NOW)).toBe(false);
    expect(untilSignInEnds(reading({ state: "ending", endsAt: "not a date" }), READ_EARLIER, NOW)).toBe(false);
  });
});

describe("the account list's own refetch", () => {
  type Query = { state: { status: string; data: AccountsReading; dataUpdatedAt: number } };
  const options = accountListQuery("claude");
  const refetchInterval = options.refetchInterval as (query: Query) => number | false;
  const refetchOnWindowFocus = options.refetchOnWindowFocus as (query: Query) => boolean | "always";

  it("waits for the next ending date only while the list reads, never while it fails", () => {
    const data = reading({ state: "ending", endsAt: new Date(Date.now() + 30 * MINUTE).toISOString() });
    expect(refetchInterval({ state: { status: "success", data, dataUpdatedAt: Date.now() } })).toBeGreaterThan(29 * MINUTE);
    expect(refetchInterval({ state: { status: "error", data, dataUpdatedAt: Date.now() } })).toBe(false);
  });

  it("reads again when the window shows after a date passed unread, never while it fails", () => {
    const data = reading({ state: "ending", endsAt: new Date(Date.now() - MINUTE).toISOString() });
    const readBefore = Date.now() - 10 * MINUTE;
    expect(refetchOnWindowFocus({ state: { status: "success", data, dataUpdatedAt: readBefore } })).toBe("always");
    expect(refetchOnWindowFocus({ state: { status: "success", data, dataUpdatedAt: Date.now() } })).toBe(false);
    expect(refetchOnWindowFocus({ state: { status: "error", data, dataUpdatedAt: readBefore } })).toBe(false);
  });
});
