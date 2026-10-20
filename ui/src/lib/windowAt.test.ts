import { describe, expect, it } from "vitest";
import cases from "../../../src-tauri/src/limits/window_at/fixtures/window-cases.json";
import type { LimitWindow, LimitsWorkspaceCredits } from "./limitsTypes";
import { shareAt, windowAt, type WindowAt } from "./windowAt";

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
  expect(at.renewed).toBe(expected.renewed);
  expect(at.reached).toBe(expected.reached);
  expect(at.idle).toBe("idle" in expected ? expected.idle : false);
  expect(at.usedText).toBe(expected.usedText);
  expect(at.meter.level).toBe(expected.meter.level);
  const fraction = (expected.meter as { fraction?: number }).fraction;
  if (at.meter.level === "blend") expect(at.meter.fraction).toBeCloseTo(fraction ?? Number.NaN, 9);
  expect(at.note.kind).toBe(expected.note.kind);
  expect(at.note.at).toBe("at" in expected.note ? expected.note.at : undefined);
  expect(at.note.relative).toBe("relative" in expected.note ? expected.note.relative : undefined);
  expect(at.note.text).toBe(expected.note.card);
}

describe("a quota window at an instant, as src-tauri/src/limits/window_at/fixtures/window-cases.json reads it", () => {
  it.each(cases.windows.map((entry) => [entry.name, entry] as const))("%s", (_, entry) => {
    expectCase(windowAt(window(entry.window), NOW, cases.timeZone), entry);
  });

  it.each(cases.shares.map((entry) => [entry.name, entry] as const))("%s", (_, entry) => {
    expectCase(shareAt(share(entry.share), NOW, cases.timeZone), entry);
  });
});
