import { describe, expect, it } from "vitest";
import rustCatalogue from "../../../../src-tauri/src/provider_catalogue/fixtures/providers.json";
import { LIMITS_PROVIDERS } from "$lib/providers";
import { LIMITS_FEATURES, type LimitsFeatures } from "./limitFeatures";

const IMPLEMENTED: { readonly [Feature in keyof LimitsFeatures]: readonly NonNullable<LimitsFeatures[Feature]>[] } = {
  signInDeadline: ["claude"],
  subscriptionTerm: ["codex"],
  bankedResets: ["codex"],
  paidResetOffer: ["codex"],
};

describe("the Limits features Rust keeps", () => {
  it("are the catalogue's rows, one for each Limits provider", () => {
    expect(Object.keys(rustCatalogue.limitsFeatures).sort()).toEqual([...LIMITS_PROVIDERS].sort());
    expect(LIMITS_FEATURES).toEqual(rustCatalogue.limitsFeatures);
  });

  it("name only an implementation the card model has, or none", () => {
    for (const provider of LIMITS_PROVIDERS) {
      expect(Object.keys(LIMITS_FEATURES[provider]).sort(), provider).toEqual(Object.keys(IMPLEMENTED).sort());
      for (const feature of Object.keys(IMPLEMENTED) as (keyof LimitsFeatures)[]) {
        const named = LIMITS_FEATURES[provider][feature];
        expect(named === null || (IMPLEMENTED[feature] as readonly string[]).includes(named), `${provider} ${feature}: ${named}`).toBe(true);
      }
    }
  });
});
