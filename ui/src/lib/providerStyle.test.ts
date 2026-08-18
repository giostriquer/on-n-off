import { describe, expect, it } from "vitest";
import { providerColor } from "./providerStyle";

describe("providerColor", () => {
  it("gives Claude its orange and every provider a colour", () => {
    expect(providerColor("claude")).toBe("#e8944a");
    for (const provider of ["claude", "codex", "antigravity", "cursor"] as const) {
      expect(providerColor(provider)).toBeTruthy();
    }
  });
});
