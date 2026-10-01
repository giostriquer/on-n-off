import { describe, expect, it } from "vitest";
import { formatResetAt } from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { hasObservations, latestObservedAt, presentCreditsSpent, presentLimitAccount, presentLimitWindow, usableAgainAt, usageLeft } from "./limitPresentation";

const NOW = Date.parse("2026-08-17T20:00:00Z");

const window: LimitWindow = {
  id: "session",
  label: "5 hour · all models",
  kind: "session",
  usedPercent: 93,
  resetsAt: "2026-08-17T18:35:00Z",
  observedAt: "2026-08-17T10:00:00Z",
};

describe("presentLimitWindow", () => {
  it("reads an elapsed window as 0%, not as a dash and not as its old figure", () => {
    const presented = presentLimitWindow(window, NOW);
    expect(presented).toEqual({
      percent: 0,
      text: "0%",
      color: undefined,
      note: `reset 1h ago · ${formatResetAt(window.resetsAt)}`,
    });
    expect(presented.note).not.toContain("93");
    expect(presented.note).not.toContain(formatResetAt(window.observedAt));
  });

  it("keeps a live window's number, colour and countdown", () => {
    const resetsAt = "2026-08-17T23:00:00Z";
    expect(presentLimitWindow({ ...window, resetsAt }, NOW)).toEqual({
      percent: 93,
      text: "93%",
      color: "var(--trip)",
      note: `resets in 3h 0m · ${formatResetAt(resetsAt)}`,
    });
  });

  it("counts down the last minute as '<1m', never 'in now'", () => {
    const resetsAt = new Date(NOW + 30_000).toISOString();
    expect(presentLimitWindow({ ...window, resetsAt }, NOW).note).toBe(`resets in <1m · ${formatResetAt(resetsAt)}`);
  });

  it("treats the reset instant itself as already elapsed", () => {
    const presented = presentLimitWindow({ ...window, resetsAt: new Date(NOW).toISOString() }, NOW);
    expect(presented.note).toMatch(/^reset just now · \w{3} \d\d:\d\d$/);
  });

  it("shows a window without a known reset as live, with no note", () => {
    const presented = presentLimitWindow({ ...window, usedPercent: 12, resetsAt: null }, NOW);
    expect(presented.text).toBe("12%");
    expect(presented.note).toBe("");
    expect(presentLimitWindow({ ...window, resetsAt: "soon" }, NOW).text).toBe("93%");
  });
});

describe("usageLeft", () => {
  const entry = (windows: LimitWindow[]): ProviderLimits => ({ provider: "codex", status: "ok", currentAccount: true, windows });
  const live = "2026-08-20T00:00:00Z";

  it("is what the most-used main window has left; model buckets do not count", () => {
    expect(
      usageLeft(
        entry([
          { ...window, id: "secondary", kind: "weekly", usedPercent: 40, resetsAt: live },
          { ...window, id: "primary", kind: "session", usedPercent: 96.5, resetsAt: live },
          { ...window, id: "extra:spark", kind: "model", usedPercent: 100, resetsAt: live },
        ]),
        NOW,
      ),
    ).toBeCloseTo(3.5);
  });

  it("counts a window whose reset has passed as renewed", () => {
    expect(usageLeft(entry([{ ...window, kind: "session", usedPercent: 99 }]), NOW)).toBe(100);
  });

  it("is unknown without a main window", () => {
    expect(usageLeft(entry([{ ...window, id: "extra:spark", kind: "model", usedPercent: 99, resetsAt: live }]), NOW)).toBeNull();
    expect(usageLeft(entry([]), NOW)).toBeNull();
  });
});

describe("usableAgainAt", () => {
  const entry = (windows: LimitWindow[]): ProviderLimits => ({ provider: "claude", status: "ok", currentAccount: false, windows });
  const soon = "2026-08-17T21:00:00Z";
  const later = "2026-08-20T00:00:00Z";
  const opus = "2026-08-24T00:00:00Z";

  it("is the reset of the last full main window, whatever a full model bucket says", () => {
    expect(usableAgainAt(entry([
      { ...window, id: "session", kind: "session", usedPercent: 100, resetsAt: soon },
      { ...window, id: "weekly", kind: "weekly", usedPercent: 100, resetsAt: later },
      { ...window, id: "opus", kind: "model", usedPercent: 100, resetsAt: opus },
    ]), NOW)).toBe(Date.parse(later));
    expect(usableAgainAt(entry([
      { ...window, id: "session", kind: "session", usedPercent: 100, resetsAt: soon },
      { ...window, id: "weekly", kind: "weekly", usedPercent: 40, resetsAt: later },
    ]), NOW)).toBe(Date.parse(soon));
  });

  it("is never, as far as is known, when a full window reports no reset", () => {
    expect(usableAgainAt(entry([{ ...window, kind: "session", usedPercent: 100, resetsAt: null }]), NOW)).toBe(Infinity);
  });

  it("ignores a full window whose reset has already passed", () => {
    expect(usableAgainAt(entry([
      { ...window, id: "session", kind: "session", usedPercent: 100 },
      { ...window, id: "weekly", kind: "weekly", usedPercent: 100, resetsAt: later },
    ]), NOW)).toBe(Date.parse(later));
  });
});

const SHARE = { limit: "25000", used: "8000", usedPercent: 32, resetsAt: null, reached: false };

const SPENT = { last7Days: 18303.44, last30Days: 20299.7, updatedAt: "2026-08-17T19:00:00Z" };

describe("presentCreditsSpent", () => {
  it("leads with the last 7 days, the app's default view, and notes the 30 days", () => {
    expect(presentCreditsSpent(SPENT)).toEqual({
      value: "18,303.4",
      note: "last 7 days · 20,299.7 in 30 days",
    });
  });

  it("keeps the note to the two windows, whenever the data was last updated", () => {
    for (const updatedAt of ["2026-08-17T19:00:00Z", "2026-08-16T22:00:00Z", null]) {
      expect(presentCreditsSpent({ ...SPENT, updatedAt }).note).toBe("last 7 days · 20,299.7 in 30 days");
    }
  });

  it("shows nothing spent as 0", () => {
    expect(presentCreditsSpent({ last7Days: 0, last30Days: 0 })).toEqual({
      value: "0",
      note: "last 7 days · 0 in 30 days",
    });
  });
});

describe("hasObservations", () => {
  const bare: ProviderLimits = { provider: "codex", status: "ok", currentAccount: true, windows: [] };

  it("counts quota windows, a credit balance, a workspace-credit share, credits spent and banked resets alike", () => {
    expect(hasObservations({ ...bare, workspaceCredits: SHARE })).toBe(true);
    expect(hasObservations({ ...bare, creditsSpent: SPENT })).toBe(true);
    expect(hasObservations(bare)).toBe(false);
    expect(hasObservations({ ...bare, windows: [window] })).toBe(true);
    expect(hasObservations({ ...bare, credits: { balance: "0", unlimited: false } })).toBe(true);
    expect(hasObservations({ ...bare, resetCredits: { availableCount: 0, nextExpiresAt: null } })).toBe(false);
    expect(hasObservations({ ...bare, resetCredits: { availableCount: 1, nextExpiresAt: null } })).toBe(true);
  });

  it("keeps a card with only a workspace-credit share as a paused refresh rather than an empty one", () => {
    const failed: ProviderLimits = { ...bare, status: "failed", message: "Refresh failed", workspaceCredits: SHARE };
    expect(presentLimitAccount(failed, "unavailable").status).toEqual({ kind: "paused" });
    expect(presentLimitAccount({ ...failed, currentAccount: false, savedProfile: true }, "unavailable").status).toEqual({ kind: "savedRefresh", detail: "Refresh failed" });
  });

  it("keeps a card with only banked resets as a paused refresh rather than an empty one", () => {
    const failed: ProviderLimits = { ...bare, status: "failed", message: "Refresh failed", resetCredits: { availableCount: 1, nextExpiresAt: null } };
    expect(presentLimitAccount(failed, "unavailable").status).toEqual({ kind: "paused" });
    expect(presentLimitAccount({ ...failed, currentAccount: false, savedProfile: true }, "unavailable").status).toEqual({ kind: "savedRefresh", detail: "Refresh failed" });
  });
});


describe.each(["failed", "unauthenticated"] as const)("saved %s usage status", status => {
  const message = "Saved usage refresh is paused.";
  it.each([
    {name:"windows", windows:[window]},
    {name:"credits", windows:[], credits:{balance:"0", unlimited:false}},
    {name:"banked resets", windows:[], resetCredits:{availableCount:1, nextExpiresAt:null}},
    {name:"a workspace-credit share", windows:[], workspaceCredits:SHARE},
  ])("quietly identifies retained $name", observation => {
    const presented = presentLimitAccount({provider:"codex", currentAccount:false, savedProfile:true, status, message, ...observation}, "fallback");
    expect(presented.message).toBeNull();
    expect(presented.status).toEqual({ kind: "savedRefresh", detail: message });
  });
  it("keeps the error visible without any retained observation", () => {
    const presented = presentLimitAccount({provider:"codex", currentAccount:false, savedProfile:true, status, message, windows:[], resetCredits:{availableCount:0, nextExpiresAt:null}}, "fallback");
    expect(presented.message).toBe(message);
    expect(presented.status).toBeNull();
  });
});

describe("which reading is remembered", () => {
  const reading: ProviderLimits = { provider: "codex", status: "ok", currentAccount: false, windows: [window] };

  it("calls a reading remembered only when it is neither the signed-in account's nor a polled saved profile's", () => {
    expect(presentLimitAccount(reading, "fallback").status).toEqual({ kind: "remembered" });
    expect(presentLimitAccount({ ...reading, savedProfile: true }, "fallback").status).toBeNull();
    expect(presentLimitAccount({ ...reading, currentAccount: true }, "fallback").status).toBeNull();
  });

  it("marks only a polled saved profile's failed read as last known", () => {
    const failed: ProviderLimits = { ...reading, status: "failed", message: "Refresh failed" };
    expect(presentLimitAccount({ ...failed, savedProfile: true }, "fallback").status).toEqual({ kind: "savedRefresh", detail: "Refresh failed" });
    expect(presentLimitAccount(failed, "fallback").status).toEqual({ kind: "remembered" });
  });
});

describe("presentLimitAccount freshness", () => {
  const claude = (overrides: Partial<ProviderLimits>): ProviderLimits => ({
    provider: "claude", status: "ok", currentAccount: true, windows: [], ...overrides,
  });
  it.each([
    ["a live signed-in read", {}, false],
    ["a saved read that answered", { currentAccount: false }, false],
    ["a signed-in read that failed", { status: "failed" }, true],
    ["a saved read that was refused", { currentAccount: false, status: "unauthenticated" }, true],
  ] as const)("calls what a card shows last known only when its read did not answer: %s", (_case, overrides, lastKnown) => {
    expect(presentLimitAccount(claude(overrides), "unavailable").lastKnown).toBe(lastKnown);
  });
});

describe("latestObservedAt", () => {
  const observedAt = (...times: string[]): ProviderLimits => ({
    provider: "codex",
    status: "ok",
    currentAccount: true,
    windows: times.map((time, index) => ({ ...window, id: `w${index}`, observedAt: time })),
  });

  it.each([
    [["2026-08-17T10:00:00Z", "2026-08-17T12:00:00Z", "2026-08-17T11:00:00Z", ""], Date.parse("2026-08-17T12:00:00Z")],
    [["", "not a time"], null],
    [[], null],
  ])("reads %j as the newest time any window says", (times, newest) => {
    expect(latestObservedAt(observedAt(...times))).toBe(newest);
  });
});
