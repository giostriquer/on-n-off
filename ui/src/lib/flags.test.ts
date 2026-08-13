import { describe, expect, it } from "vitest";
import { DEFAULT_FLAGS, flagOn, mergeFlags } from "./flags";

describe("flags", () => {
  it("defaults master cut off", () => {
    expect(DEFAULT_FLAGS.masterCut).toBe(false);
    expect(flagOn(DEFAULT_FLAGS, "masterCut")).toBe(false);
  });

  it("merges a partial overlay onto defaults", () => {
    expect(mergeFlags({ masterCut: true })).toEqual({ masterCut: true });
    expect(mergeFlags(null)).toEqual(DEFAULT_FLAGS);
    expect(mergeFlags(undefined)).toEqual(DEFAULT_FLAGS);
  });
});
