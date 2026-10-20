import { describe, expect, it } from "vitest";
import cases from "../../../src-tauri/src/limits/window_at/fixtures/window-cases.json";
import type { LimitWindow, LimitsWorkspaceCredits } from "./limitsTypes";
import { shareAt, usageFillStyle, usageMeterColor, usageTextColor, windowAt, type WindowAt } from "./windowAt";

type Case = (typeof cases.windows)[number] | (typeof cases.shares)[number];

const NOW = Date.parse(cases.now);

function window(input: (typeof cases.windows)[number]["window"]): LimitWindow {
  return {
    id: "w",
    label: "Weekly · all models",
    kind: "weekly",
    usedPercent: input.usedPercent ?? Number.NaN,
    resetsAt: "resetsAt" in input ? input.resetsAt : undefined,
    observedAt: "",
    startsOnFirstMessage: "startsOnFirstMessage" in input ? input.startsOnFirstMessage : undefined,
  };
}

function share(input: (typeof cases.shares)[number]["share"]): LimitsWorkspaceCredits {
  return {
    limit: "25000",
    used: "8000",
    usedPercent: input.usedPercent ?? Number.NaN,
    resetsAt: "resetsAt" in input ? input.resetsAt : undefined,
    reached: false,
  };
}

function expectCase(at: WindowAt, expected: Case) {
  expect(at.usedNow).toBe(expected.usedNow);
  expect(at.usedFigure).toBe(expected.usedFigure);
  expect(at.reached).toBe(expected.reached);
  expect(at.usedText).toBe(expected.usedText);
  expect(at.meter.level).toBe(expected.meter.level);
  const fraction = (expected.meter as { fraction?: number }).fraction;
  if (at.meter.level === "blend") expect(at.meter.fraction).toBeCloseTo(fraction ?? Number.NaN, 9);
  const note = expected.note as { kind: string; when?: string; relative?: string; card: string };
  expect(at.note).toEqual({
    kind: note.kind,
    ...(note.when === undefined ? {} : { when: note.when }),
    ...(note.relative === undefined ? {} : { relative: note.relative }),
    text: note.card,
  });
}

describe("a quota window at an instant, as src-tauri/src/limits/window_at/fixtures/window-cases.json reads it", () => {
  it.each(cases.windows.map((entry) => [entry.name, entry] as const))("%s", (_, entry) => {
    expectCase(windowAt(window(entry.window), NOW, cases.timeZone), entry);
  });

  it.each(cases.shares.map((entry) => [entry.name, entry] as const))("%s", (_, entry) => {
    expectCase(shareAt(share(entry.share), NOW, cases.timeZone), entry);
  });
});

describe("usageMeterColor and usageTextColor", () => {
  it("draw each meter level in one ink, never the warning amber", () => {
    expect(usageMeterColor("red", { level: "base" })).toBe("red");
    expect(usageMeterColor("red", { level: "unreadable" })).toBe("red");
    expect(usageMeterColor("red", { level: "blend", fraction: 0.5 })).toBe("color-mix(in srgb, red, var(--trip) 50.0%)");
    expect(usageMeterColor("red", { level: "trip" })).toBe("var(--trip)");
    expect(usageFillStyle("claude", { level: "base" })).toEqual({ background: "#d97757", backgroundColor: "#d97757" });
    expect(usageTextColor({ level: "blend", fraction: 0.99 })).toBeUndefined();
    expect(usageTextColor({ level: "trip" })).toBe("var(--trip)");
  });
});
