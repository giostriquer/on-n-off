import { describe, expect, it } from "vitest";
import {
  formatClock,
  formatPrice,
  formatObservedAt,
  formatShortDate,
  formatResetAt,
  formatResetIn,
  hasElapsed,
  usageFillStyle,
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

describe("formatShortDate", () => {
  it("names the day of a date weeks away, where a weekday alone would be ambiguous", () => {
    expect(formatShortDate("2026-08-29T15:00:00Z", { timeZone: "UTC" })).toBe("Aug 29");
    expect(formatShortDate("2026-08-29T23:30:00Z", { timeZone: "America/Sao_Paulo" })).toBe("Aug 29");
    expect(formatShortDate("2026-08-30T01:30:00Z", { timeZone: "America/Sao_Paulo" })).toBe("Aug 29");
    expect(formatShortDate("2027-01-04T12:00:00Z", { timeZone: "UTC", withYear: true })).toBe("Jan 4, 2027");
    const newYearsEve = "2026-12-31T23:30:00Z";
    const justAfter = Date.parse("2027-01-01T01:00:00Z");
    expect(formatShortDate(newYearsEve, { timeZone: "UTC", yearUnlessSameAs: justAfter })).toBe("Dec 31, 2026");
    expect(formatShortDate(newYearsEve, { timeZone: "America/Sao_Paulo", yearUnlessSameAs: justAfter })).toBe("Dec 31");
    expect(formatShortDate(null)).toBe("");
    expect(formatShortDate("not a date")).toBe("");
  });
});

describe("formatPrice", () => {
  it("divides by the minor units the currency actually has", () => {
    expect(formatPrice({ currency: "USD", amountMinorUnits: 800 })).toBe("$8.00");
    expect(formatPrice({ currency: "EUR", amountMinorUnits: 1250 })).toBe("€12.50");
    expect(formatPrice({ currency: "JPY", amountMinorUnits: 1200 })).toBe("¥1,200");
    expect(formatPrice({ currency: "KRW", amountMinorUnits: 12000 })).toBe("₩12,000");
    expect(formatPrice({ currency: "BHD", amountMinorUnits: 3000 })).toBe("BHD 3.000");
    expect(formatPrice({ currency: "BRL", amountMinorUnits: 4250 })).toBe("R$42.50");
  });

  it("shows the number beside a code with no symbol, and never disappears on a bad one", () => {
    expect(formatPrice({ currency: "ZZZ", amountMinorUnits: 800 })).toBe("ZZZ 8.00");
    expect(formatPrice({ currency: "US", amountMinorUnits: 800 })).toBe("US 8");
  });
});
