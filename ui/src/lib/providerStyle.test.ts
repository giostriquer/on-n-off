import { describe, expect, it } from "vitest";
import { ALL_AGENTS } from "./appSettings";
import { providerColor } from "./providerStyle";

describe("providerColor", () => {
  it("gives every provider a colour to paint its bars with", () => {
    for (const provider of ALL_AGENTS) {
      expect(providerColor(provider), provider).toBeTruthy();
    }
  });
});
