import { describe, expect, it } from "vitest";
import { windowAt } from "$lib/windowAt";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { latestObservedAt, presentCreditsSpent, presentLimitWindow, usableAgainAt, usageLeft } from "./limitPresentation";

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
  it("draws a window as it reads now: its meter's fill, figure and level, its text and note", () => {
    const live = { ...window, resetsAt: "2026-08-17T23:00:00Z" };
    expect(presentLimitWindow(live, NOW)).toEqual({
      usedNow: 93,
      usedFigure: 93,
      meter: { level: "trip" },
      text: "93%",
      note: windowAt(live, NOW).note.text,
    });
  });

  it("keeps an unreadable window unreadable: no fill, no figure, a dash", () => {
    expect(presentLimitWindow({ ...window, usedPercent: 101, resetsAt: null }, NOW)).toEqual({
      usedNow: null,
      usedFigure: null,
      meter: { level: "unreadable" },
      text: "—",
      note: "Reset time unavailable",
    });
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
