import { describe, expect, it } from "vitest";
import { codexSubscriptionTerm } from "./codexSubscriptionTerm";

const DAY = 86_400_000;
const NOW = Date.parse("2026-09-14T12:00:00Z");
const term = (days: number, willRenew: boolean, note: "cancelled" | "planChange" | "pastDue" | null = null) =>
  ({ activeUntil: new Date(NOW + days * DAY).toISOString(), willRenew, note, checkedAt: new Date(NOW - DAY).toISOString() });
const token = (days: number) => ({ date: new Date(NOW + days * DAY).toISOString(), checkedAt: null });

describe("codexSubscriptionTerm", () => {
  it.each([
    ["renews", term(30, true), "renews", "Auto-renew", "neutral", 0],
    ["ends, far off", term(30, false), "ends", "No renewal", "neutral", 0],
    ["ends, halfway through the last week", term(3.5, false), "ends", "No renewal", "warning", 0.5],
    ["ended", term(-1, false), "ended", "No renewal", "expired", 0],
    ["renewal overdue", term(-1, true), "renewalOverdue", "Unconfirmed", "neutral", 0],
  ] as const)("%s", (_case, value, state, label, tone, progress) => {
    const shown = codexSubscriptionTerm(value, token(40), NOW)!;
    expect([shown.state, shown.label, shown.tone]).toEqual([state, label, tone]);
    expect(shown.progress).toBeCloseTo(progress);
    expect(shown.name).toBe(`Subscription status: ${label}`);
    expect(shown.checked).toMatch(/^Checked /);
  });
  it("falls back to the token date, and to nothing once that has passed or is unreadable", () => {
    const fallback = codexSubscriptionTerm(null, token(40), NOW)!;
    expect([fallback.state, fallback.label, fallback.name]).toEqual(["tokenOnly", "Until Oct 24", "Subscription paid through Oct 24"]);
    expect(fallback.caveat).toMatch(/Renewal status unknown/);
    expect(fallback.checked).toBeNull();
    expect(codexSubscriptionTerm(null, token(-1), NOW)).toBeNull();
    expect(codexSubscriptionTerm(null, { date: "invalid", checkedAt: null }, NOW)).toBeNull();
    expect(codexSubscriptionTerm(null, null, NOW)).toBeNull();
  });
  it("prefers the billing term to the token date", () => {
    const shown = codexSubscriptionTerm(term(3, false), token(40), NOW)!;
    expect([shown.state, shown.tone]).toEqual(["ends", "warning"]);
    expect(shown.headline).toMatch(/^Expires .*Sep 17/);
  });
  it("warns from exactly seven days out, not a minute before", () => {
    expect(codexSubscriptionTerm(term(7 + 1 / 1440, false), null, NOW)).toMatchObject({ tone: "neutral", progress: 0 });
    expect(codexSubscriptionTerm(term(7, false), null, NOW)).toMatchObject({ tone: "warning", progress: 0 });
    expect(codexSubscriptionTerm(term(6, false), null, NOW)!.progress).toBeCloseTo(1 / 7);
  });
  it("counts whole days, and only for a plan that ends", () => {
    expect(codexSubscriptionTerm(term(3.5, false), null, NOW)!.countdown).toBe("4 days remaining");
    expect(codexSubscriptionTerm(term(1, false), null, NOW)!.countdown).toBe("1 day remaining");
    expect(codexSubscriptionTerm(term(6, false), null, NOW)!.countdown).toBe("6 days remaining");
    expect(codexSubscriptionTerm(term(30, true), null, NOW)).toMatchObject({ countdown: null, caveat: null });
    expect(codexSubscriptionTerm(term(30, true), null, NOW)!.headline).toMatch(/^Renews /);
    expect(codexSubscriptionTerm(term(-1, false), null, NOW)).toMatchObject({ countdown: null });
    expect(codexSubscriptionTerm(term(-1, false), null, NOW)!.headline).toMatch(/^Recorded expiry: /);
  });
  it("counts the days a plan that ends has left, and names the note", () => {
    expect(codexSubscriptionTerm(term(0.5, false, "cancelled"), null, NOW)).toMatchObject({
      countdown: "Less than 1 day remaining", note: "Cancelled: the plan ends with this period.", caveat: null,
    });
    expect(codexSubscriptionTerm(term(2, false), null, NOW)!.countdown).toBe("2 days remaining");
    expect(codexSubscriptionTerm(term(2, true, "pastDue"), null, NOW)).toMatchObject({ countdown: null, note: "A payment is overdue." });
    expect(codexSubscriptionTerm(term(-1, true), null, NOW)!.caveat).toBe("Current subscription status needs confirmation.");
  });
});
