import { describe, expect, it } from "vitest";
import { formatResetAt } from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { presentLimitWindow, visibleLimitWindows } from "./limitPresentation";

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
      tone: "calm",
      text: "0%",
      color: undefined,
      note: `reset 1h ago · ${formatResetAt(window.resetsAt)}`,
    });
    // The renewed window is zero; the figure it held before the reset is gone, not recited.
    expect(presented.note).not.toContain("93");
    // The clock time is the reset's, not the observation's.
    expect(presented.note).not.toContain(formatResetAt(window.observedAt));
  });

  it("keeps a live window's number, tone and countdown", () => {
    const resetsAt = "2026-08-17T23:00:00Z";
    expect(presentLimitWindow({ ...window, resetsAt }, NOW)).toEqual({
      percent: 93,
      tone: "trip",
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
