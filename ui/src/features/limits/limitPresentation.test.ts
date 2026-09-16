import { describe, expect, it } from "vitest";
import { formatResetAt } from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { hasObservations, presentLimitAccount, presentLimitWindow, usageLeft, visibleLimitWindows } from "./limitPresentation";

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
    // The renewed window is zero; the figure it held before the reset is gone, not recited.
    expect(presented.note).not.toContain("93");
    // The clock time is the reset's, not the observation's.
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

describe("visibleLimitWindows", () => {
  it("keeps a longer model name that only ends with a hidden Codex model name", () => {
    const entry: ProviderLimits = {
      provider: "codex",
      status: "ok",
      currentAccount: true,
      windows: [
        { ...window, id: "extra:reserve", label: "Weekly · GPT-Reserve" },
        { ...window, id: "extra:team-reserve", label: "Weekly · Team GPT-Reserve" },
      ],
    };

    expect(visibleLimitWindows(entry).map(({ id }) => id)).toEqual(["extra:team-reserve"]);
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

describe("hasObservations", () => {
  const bare: ProviderLimits = { provider: "codex", status: "ok", currentAccount: true, windows: [] };

  it("counts quota windows, a credit balance and banked resets alike", () => {
    expect(hasObservations(bare)).toBe(false);
    expect(hasObservations({ ...bare, windows: [window] })).toBe(true);
    expect(hasObservations({ ...bare, credits: { balance: "0", unlimited: false } })).toBe(true);
    expect(hasObservations({ ...bare, resetCredits: { availableCount: 0, nextExpiresAt: null } })).toBe(true);
  });

  it("lets a caller count only the windows it shows", () => {
    expect(hasObservations({ ...bare, windows: [window] }, [])).toBe(false);
  });

  it("keeps a card with only banked resets as a paused refresh rather than an empty one", () => {
    const failed: ProviderLimits = { ...bare, status: "failed", message: "Refresh failed", resetCredits: { availableCount: 1, nextExpiresAt: null } };
    expect(presentLimitAccount(failed, "unavailable").refreshPaused).toBe(true);
    expect(presentLimitAccount({ ...failed, currentAccount: false }, "unavailable").remembered).toBe(true);
  });
});

