import { describe, expect, it } from "vitest";
import { PROVIDER_IDS } from "./providers";
import { providerColor } from "./providerStyle";

describe("providerColor", () => {
  it("gives every provider a colour to paint its bars with", () => {
    for (const provider of PROVIDER_IDS) {
      expect(providerColor(provider), provider).toBeTruthy();
    }
  });
});
