import { describe, expect, it } from "vitest";
import rustCatalogue from "../../../../src-tauri/src/provider_catalogue/fixtures/providers.json";
import { LIMITS_PROVIDERS } from "$lib/providers";
import { LIMITS_FEATURES, LIMITS_IMPLEMENTATIONS } from "./limitFeatures";

const FEATURES = Object.keys(LIMITS_IMPLEMENTATIONS) as (keyof typeof LIMITS_IMPLEMENTATIONS)[];

describe("the Limits features Rust keeps", () => {
  it("are the catalogue's rows, one for each Limits provider", () => {
    expect(Object.keys(rustCatalogue.limitsFeatures).sort()).toEqual([...LIMITS_PROVIDERS].sort());
    expect(LIMITS_FEATURES).toEqual(rustCatalogue.limitsFeatures);
  });

  it("name only an implementation the card model has, or none", () => {
    for (const provider of LIMITS_PROVIDERS) {
      const row: Record<string, string | null> = rustCatalogue.limitsFeatures[provider];
      expect(Object.keys(row).sort(), provider).toEqual([...FEATURES].sort());
      for (const feature of FEATURES) {
        const named = row[feature];
        const known: readonly string[] = LIMITS_IMPLEMENTATIONS[feature];
        expect(named === null || known.includes(named), `${provider} ${feature}: ${named}`).toBe(true);
      }
    }
  });
});
