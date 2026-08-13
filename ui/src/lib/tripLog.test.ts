import { describe, expect, it, vi } from "vitest";
import { prependTrip, stampNow, tripTagClass } from "./tripLog";

describe("tripLog", () => {
  it("stamps HH:MM", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 7, 13, 9, 7));
    expect(stampNow()).toBe("09:07");
    vi.useRealTimers();
  });

  it("keeps the newest seven entries", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 7, 13, 12, 0));
    const log = prependTrip(
      [
        { at: "11:00", tag: "ON", text: "old" },
        { at: "10:00", tag: "OFF", text: "older" },
      ],
      "SYNC",
      "scanned Claude config · 3 items",
    );
    expect(log[0]).toEqual({ at: "12:00", tag: "SYNC", text: "scanned Claude config · 3 items" });
    expect(log).toHaveLength(3);
    const full = Array.from({ length: 10 }, (_, index) =>
      prependTrip([], "ON", `item ${index}`),
    ).flat();
    expect(prependTrip(full, "TRIP", "boom")).toHaveLength(7);
    vi.useRealTimers();
  });

  it("colors trip and on tags", () => {
    expect(tripTagClass("TRIP")).toContain("--trip");
    expect(tripTagClass("ON")).toContain("--brass");
    expect(tripTagClass("SYNC")).toContain("--well");
  });
});
