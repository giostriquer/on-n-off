import { describe, expect, it } from "vitest";
import { formatAgo } from "./timeFormat";

const NOW = Date.parse("2026-08-24T20:00:00Z");

describe("formatAgo", () => {
  it("says how long ago an instant was, in the coarsest unit that fits", () => {
    expect(formatAgo("2026-08-24T19:59:30Z", NOW)).toBe("just now");
    expect(formatAgo("2026-08-24T19:55:00Z", NOW)).toBe("5m ago");
    expect(formatAgo("2026-08-24T17:10:00Z", NOW)).toBe("2h ago");
    expect(formatAgo("2026-08-21T20:00:00Z", NOW)).toBe("3d ago");
  });

  it("floors at every unit boundary", () => {
    expect(formatAgo("2026-08-24T20:00:00Z", NOW)).toBe("just now");
    expect(formatAgo("2026-08-24T19:59:00Z", NOW)).toBe("1m ago");
    expect(formatAgo("2026-08-24T19:00:30Z", NOW)).toBe("59m ago");
    expect(formatAgo("2026-08-24T19:00:00Z", NOW)).toBe("1h ago");
    expect(formatAgo("2026-08-23T20:00:00Z", NOW)).toBe("1d ago");
    expect(formatAgo("2026-08-19T21:00:00Z", NOW)).toBe("4d ago");
  });

  it("treats a future instant as just now and unreadable input as nothing", () => {
    expect(formatAgo("2026-08-24T20:00:01Z", NOW)).toBe("just now");
    expect(formatAgo("not a date", NOW)).toBe("");
    expect(formatAgo(null, NOW)).toBe("");
    expect(formatAgo(undefined, NOW)).toBe("");
  });
});
