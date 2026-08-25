import { describe, expect, it } from "vitest";
import {
  formatClock,
  formatObservedAt,
  formatResetAt,
  formatResetIn,
  hasElapsed,
  formatUsedPercent,
  planLabel,
  usageTone,
} from "./limitsFormat";

const NOW = Date.parse("2026-08-17T00:00:00Z");

describe("formatResetIn", () => {
  it("shows days and hours when more than a day away", () => {
    expect(formatResetIn("2026-08-19T02:04:00Z", NOW)).toBe("2d 2h");
  });

  it("shows hours and minutes under a day", () => {
    expect(formatResetIn("2026-08-17T04:05:30Z", NOW)).toBe("4h 5m");
  });

  it("shows minutes under an hour, '<1m' inside the last minute, and nothing once elapsed", () => {
    expect(formatResetIn("2026-08-17T00:12:00Z", NOW)).toBe("12m");
    expect(formatResetIn("2026-08-17T00:01:00Z", NOW)).toBe("1m");
    expect(formatResetIn("2026-08-17T00:00:20Z", NOW)).toBe("<1m");
    expect(formatResetIn("2026-08-17T00:00:00Z", NOW)).toBe("");
    expect(formatResetIn("2026-08-16T23:00:00Z", NOW)).toBe("");
  });

  it("is empty for missing or unparsable instants", () => {
    expect(formatResetIn(null, NOW)).toBe("");
    expect(formatResetIn(undefined, NOW)).toBe("");
    expect(formatResetIn("soon", NOW)).toBe("");
  });
});

describe("formatResetAt", () => {
  it("renders weekday and 24h time in the given zone", () => {
    expect(formatResetAt("2026-08-19T02:04:00Z", "UTC")).toBe("Wed 02:04");
    expect(formatResetAt("2026-08-19T02:04:00Z", "America/Sao_Paulo")).toBe("Tue 23:04");
  });

  it("is empty for missing or unparsable instants", () => {
    expect(formatResetAt(null, "UTC")).toBe("");
    expect(formatResetAt("nope", "UTC")).toBe("");
  });
});

describe("formatObservedAt", () => {
  it("includes the calendar date so old observations cannot look recent", () => {
    expect(formatObservedAt("2026-08-19T02:04:00Z", "UTC")).toBe("Aug 19, 2026, 02:04");
    expect(formatObservedAt("2026-08-19T02:04:00Z", "America/Sao_Paulo")).toBe("Aug 18, 2026, 23:04");
  });

  it("is empty for missing or unparsable instants", () => {
    expect(formatObservedAt(null, "UTC")).toBe("");
    expect(formatObservedAt("nope", "UTC")).toBe("");
  });
});

describe("hasElapsed", () => {
  it("is true only for a known instant at or before now", () => {
    expect(hasElapsed("2026-08-16T23:59:59Z", NOW)).toBe(true);
    expect(hasElapsed("2026-08-17T00:00:00Z", NOW)).toBe(true);
    expect(hasElapsed("2026-08-17T00:00:01Z", NOW)).toBe(false);
    expect(hasElapsed(null, NOW)).toBe(false);
    expect(hasElapsed("nope", NOW)).toBe(false);
  });
});

describe("formatClock", () => {
  it("renders 24h clock time in the given zone and is empty when unknown", () => {
    expect(formatClock("2026-08-17T20:05:00Z", "UTC")).toBe("20:05");
    expect(formatClock("2026-08-17T20:05:00Z", "America/Sao_Paulo")).toBe("17:05");
    expect(formatClock(null, "UTC")).toBe("");
    expect(formatClock("later", "UTC")).toBe("");
  });
});

describe("usageTone", () => {
  it("is calm below 70, warn from 70, trip from 90", () => {
    expect(usageTone(0)).toBe("calm");
    expect(usageTone(69.9)).toBe("calm");
    expect(usageTone(70)).toBe("warn");
    expect(usageTone(89.9)).toBe("warn");
    expect(usageTone(90)).toBe("trip");
    expect(usageTone(100)).toBe("trip");
  });
});

describe("formatUsedPercent", () => {
  it("rounds to whole percent and keeps tiny non-zero use visible", () => {
    expect(formatUsedPercent(12)).toBe("12%");
    expect(formatUsedPercent(7.4)).toBe("7%");
    expect(formatUsedPercent(0.4)).toBe("<1%");
    expect(formatUsedPercent(0)).toBe("0%");
    expect(formatUsedPercent(100)).toBe("100%");
  });
});

describe("planLabel", () => {
  it("capitalises known plan ids and passes others through", () => {
    expect(planLabel("max")).toBe("Max");
    expect(planLabel("pro")).toBe("Pro");
    expect(planLabel("team")).toBe("Team");
    expect(planLabel("plus")).toBe("Plus");
    expect(planLabel("enterprise_x")).toBe("Enterprise x");
    expect(planLabel(null)).toBe("");
    expect(planLabel(undefined)).toBe("");
  });
});
