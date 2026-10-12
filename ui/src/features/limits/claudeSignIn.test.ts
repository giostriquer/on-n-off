import { describe, expect, it } from "vitest";
import { claudeSignIn } from "./claudeSignIn";

const DAY = 86_400_000;
const NOW = Date.parse("2026-10-08T12:00:00Z");
const at = (days: number) => new Date(NOW + days * DAY).toISOString();

describe("claudeSignIn", () => {
  it("stays out of the way until the sign-in's last seven days", () => {
    expect(claudeSignIn(at(7.01), NOW)).toBeNull();
    expect(claudeSignIn(undefined, NOW)).toBeNull();
    expect(claudeSignIn(null, NOW)).toBeNull();
    expect(claudeSignIn("not a date", NOW)).toBeNull();
  });

  it("warns through the last seven days, filling as the end nears", () => {
    expect(claudeSignIn(at(7), NOW)).toMatchObject({
      state: "ending", tone: "warning", progress: 0,
      label: "Sign-in ends Oct 15", name: "Claude sign-in ends Oct 15", countdown: "7 days remaining",
    });
    expect(claudeSignIn(at(3.5), NOW)).toMatchObject({ progress: 0.5, countdown: "4 days remaining" });
    expect(claudeSignIn(at(1), NOW)).toMatchObject({ countdown: "1 day remaining" });
    expect(claudeSignIn(at(0.5), NOW)).toMatchObject({ countdown: "Less than 1 day remaining" });
  });

  it("says the sign-in ended once its date passes", () => {
    expect(claudeSignIn(at(-1), NOW)).toMatchObject({
      state: "ended", tone: "expired", progress: 0,
      label: "Sign-in ended", name: "Claude sign-in ended Oct 7", countdown: null,
    });
    expect(claudeSignIn(at(0), NOW)).toMatchObject({ state: "ended" });
    expect(claudeSignIn(at(-400), NOW)).toMatchObject({ name: "Claude sign-in ended Sep 3, 2025" });
  });

  it("tells the exact local time and what comes next", () => {
    const ending = claudeSignIn(at(3), NOW)!;
    expect(ending.headline).toMatch(/^Claude ends this sign-in Oct 11, 2026, \d{1,2}:\d{2}/);
    expect(ending.headline).not.toMatch(/GMT|UTC/);
    expect(ending.next).toBe("Then Claude Code asks you to sign in again.");
    const ended = claudeSignIn(at(-1), NOW)!;
    expect(ended.headline).toMatch(/^Claude ended this sign-in Oct 7, 2026, \d{1,2}:\d{2}/);
    expect(ended.next).toBe("Sign in again to use this account.");
    expect(ended.message).toBe("Claude ended this sign-in on Oct 7. Sign in again to use this account.");
    expect(ending.message).toBeNull();
  });
});
