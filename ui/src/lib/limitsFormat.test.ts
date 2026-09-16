import { describe, expect, it } from "vitest";
import {
  formatClock,
  formatObservedAt,
  formatShortDate,
  formatResetAt,
  formatResetIn,
  hasElapsed,
  formatUsedPercent,
  planLabel,
  usageFillColor,
  usageMeterColor,
  usageTextColor,
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

describe("usageMeterColor", () => {
  // The bar used to step to `--warn` at 70 %, an amber lighter than the accents it replaced, so a
  // filling meter went paler and yellower as it ran out. It now hardens toward `--trip` instead,
  // matching the side notch. These cases are the ones that would catch a regression to a step.
  it("holds the base colour while there is room", () => {
    expect(usageMeterColor("red", 0)).toBe("red");
    expect(usageMeterColor("red", 69.9)).toBe("red");
    expect(usageMeterColor("red", 70)).toBe("red");
  });

  it("is fully tripped from 90", () => {
    expect(usageMeterColor("red", 90)).toBe("var(--trip)");
    expect(usageMeterColor("red", 100)).toBe("var(--trip)");
  });

  it("never passes through the warning amber", () => {
    for (let percent = 0; percent <= 100; percent += 0.5) {
      expect(usageMeterColor("red", percent)).not.toContain("--warn");
    }
  });

  it("eases, so a quarter through the band is half the way to red", () => {
    expect(usageMeterColor("red", 75)).toBe("color-mix(in srgb, red, var(--trip) 50.0%)");
    // A linear blend would put 25 % here; this is the assertion that pins the easing.
    expect(usageMeterColor("red", 75)).not.toContain("25.0%");
  });

  it("moves monotonically toward red across the band", () => {
    const share = (percent: number) =>
      Number(/var\(--trip\) ([\d.]+)%/.exec(usageMeterColor("red", percent))?.[1] ?? 0);
    let previous = -1;
    for (let percent = 70.5; percent < 90; percent += 0.5) {
      const now = share(percent);
      expect(now).toBeGreaterThan(previous);
      previous = now;
    }
  });
});

describe("usageFillColor and usageTextColor", () => {
  it("fills with the provider accent until the band", () => {
    expect(usageFillColor("claude", 50)).toBe("#d97757");
    expect(usageFillColor("claude", 95)).toBe("var(--trip)");
  });

  it("leaves the figure its ordinary colour until the window is spent", () => {
    expect(usageTextColor(50)).toBeUndefined();
    expect(usageTextColor(70)).toBeUndefined();
    // Blending the page ink toward red would wash the figure out rather than sharpen it.
    expect(usageTextColor(80)).toBeUndefined();
    expect(usageTextColor(90)).toBe("var(--trip)");
    expect(usageTextColor(95)).toBe("var(--trip)");
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

it("distinguishes Codex Pro tiers without relabeling another provider's Pro", () => {
  expect(planLabel("pro", "codex")).toBe("Pro ×20");
  for (const value of ["prolite", "pro_lite", "pro-lite", " Pro Lite "]) {
    expect(planLabel(value, "codex")).toBe("Pro ×5");
  }
  expect(planLabel("pro", "claude")).toBe("Pro");
  expect(planLabel("future_plan", "codex")).toBe("Future plan");
});

describe("formatShortDate", () => {
  it("names the day of a date weeks away, where a weekday alone would be ambiguous", () => {
    expect(formatShortDate("2026-08-29T15:00:00Z", { timeZone: "UTC" })).toBe("Aug 29");
    expect(formatShortDate("2026-08-29T23:30:00Z", { timeZone: "America/Sao_Paulo" })).toBe("Aug 29");
    expect(formatShortDate("2026-08-30T01:30:00Z", { timeZone: "America/Sao_Paulo" })).toBe("Aug 29");
    expect(formatShortDate("2027-01-04T12:00:00Z", { timeZone: "UTC", withYear: true })).toBe("Jan 4, 2027");
    // The year appears only outside the year of `yearUnlessSameAs`, judged in the same time zone.
    const newYearsEve = "2026-12-31T23:30:00Z";
    const justAfter = Date.parse("2027-01-01T01:00:00Z");
    expect(formatShortDate(newYearsEve, { timeZone: "UTC", yearUnlessSameAs: justAfter })).toBe("Dec 31, 2026");
    expect(formatShortDate(newYearsEve, { timeZone: "America/Sao_Paulo", yearUnlessSameAs: justAfter })).toBe("Dec 31");
    expect(formatShortDate(null)).toBe("");
    expect(formatShortDate("not a date")).toBe("");
  });
});
