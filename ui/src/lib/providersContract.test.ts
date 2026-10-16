import { describe, expect, expectTypeOf, it } from "vitest";
import rustCatalogue from "../../../src-tauri/src/provider_catalogue/fixtures/providers.json";
import {
  ACCOUNT_PROVIDERS,
  LIMITS_PROVIDERS,
  PROVIDER_IDS,
  RAIL_ORDER,
  USAGE_PROVIDERS,
  providerBinary,
  providerLabel,
  type AccountProvider,
  type LimitsProvider,
} from "./providers";
import type { AgentId } from "./types";

type Row = (typeof rustCatalogue.providers)[AgentId];
const rows: Record<string, Row> = rustCatalogue.providers;
const withFact = (fact: (row: Row) => boolean) => rustCatalogue.tabOrder.filter((key) => fact(rows[key]));

describe("the provider catalogue Rust keeps", () => {
  it("names exactly the providers the UI's type names, each once in each order", () => {
    expectTypeOf<AgentId>().toEqualTypeOf<keyof typeof rustCatalogue.providers>();
    const keys = Object.keys(rustCatalogue.providers).sort();
    expect([...rustCatalogue.tabOrder].sort()).toEqual(keys);
    expect([...rustCatalogue.railOrder].sort()).toEqual(keys);
  });

  it("is the order the UI lists its tabs and the notch rail in", () => {
    expect(PROVIDER_IDS).toEqual(rustCatalogue.tabOrder);
    expect(RAIL_ORDER).toEqual(rustCatalogue.railOrder);
  });

  it("gives each provider the name and command the UI shows", () => {
    for (const id of PROVIDER_IDS) {
      expect(providerLabel(id), id).toBe(rows[id].displayName);
      expect(providerBinary(id), id).toBe(rows[id].binary);
    }
  });

  it("gives the Limits and saved-account lists, and the types drawn from them, exactly the providers whose rows have those screens", () => {
    expectTypeOf<LimitsProvider>().toExtend<AgentId>();
    expectTypeOf<AccountProvider>().toExtend<AgentId>();
    expect(LIMITS_PROVIDERS).toEqual(withFact((row) => row.limitsReader));
    expect(ACCOUNT_PROVIDERS).toEqual(withFact((row) => row.savedAccounts));
    expect(USAGE_PROVIDERS).toEqual(withFact((row) => row.usage !== null));
    for (const id of USAGE_PROVIDERS) expect(rows[id].usage, id).toBe(id);
  });
});
