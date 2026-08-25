import { afterEach, describe, expect, it, vi } from "vitest";
import { readCollapsed, writeCollapsed } from "./collapsed";

const KEY = "on-n-off.github.collapsed";

describe("collapsed sections memory", () => {
  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("round-trips the folded sections", () => {
    writeCollapsed(new Set(["mine", "assigned"]));
    expect([...readCollapsed()].sort()).toEqual(["assigned", "mine"]);
  });

  it("ignores corrupt, non-array, and unknown stored values", () => {
    localStorage.setItem(KEY, "{nope");
    expect(readCollapsed().size).toBe(0);
    localStorage.setItem(KEY, JSON.stringify({ mine: true }));
    expect(readCollapsed().size).toBe(0);
    localStorage.setItem(KEY, JSON.stringify(["mine", "constructor", 7, "reviewRequested"]));
    expect([...readCollapsed()].sort()).toEqual(["mine", "reviewRequested"]);
  });

  it("survives storage that throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(readCollapsed().size).toBe(0);
    expect(() => writeCollapsed(new Set(["mine"]))).not.toThrow();
  });
});
