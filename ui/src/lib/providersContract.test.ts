import { describe, expect, expectTypeOf, it } from "vitest";
import rustCatalogue from "../../../src-tauri/src/provider_catalogue/fixtures/providers.json";
import type { AccountProvider } from "./accountTypes";
import { ALL_AGENTS } from "./appSettings";
import { NOTCH_PROVIDER_ORDER } from "./notchTypes";
import { LOCKED_AGENTS } from "./session";
import { LIMITS_PROVIDERS } from "./sharedReads/limits";
import type { AgentId } from "./types";
import { PROVIDERS, providerLabel } from "./usageMerge";

const rows = rustCatalogue.providers as Record<string, (typeof rustCatalogue.providers)[AgentId]>;
const withFact = (fact: (row: (typeof rows)[string]) => boolean) => rustCatalogue.tabOrder.filter((key) => fact(rows[key]!));

describe("the provider catalogue Rust keeps", () => {
  it("names exactly the providers the UI's type names, each once in its tab order", () => {
    expectTypeOf<AgentId>().toEqualTypeOf<keyof typeof rustCatalogue.providers>();
    expect([...rustCatalogue.tabOrder].sort()).toEqual(Object.keys(rustCatalogue.providers).sort());
    expect([...rustCatalogue.railOrder].sort()).toEqual(Object.keys(rustCatalogue.providers).sort());
  });

  it("lists the providers in the orders the UI keeps", () => {
    expect(ALL_AGENTS).toEqual(rustCatalogue.tabOrder);
    expect(LOCKED_AGENTS.map((agent) => agent.id)).toEqual(rustCatalogue.tabOrder);
    expect(NOTCH_PROVIDER_ORDER).toEqual(rustCatalogue.railOrder);
  });

  it("gives each provider the name and screens the UI writes out", () => {
    for (const key of rustCatalogue.tabOrder) {
      expect(providerLabel(key as AgentId), key).toBe(rows[key]!.displayName);
      expect(LOCKED_AGENTS.find((agent) => agent.id === key)?.displayName, key).toBe(rows[key]!.displayName);
    }
    expect(LIMITS_PROVIDERS).toEqual(withFact((row) => row.limitsReader));
    expect(PROVIDERS).toEqual(withFact((row) => row.usage !== null));
    const accounts = { claude: true, codex: true } satisfies Record<AccountProvider, true>;
    expect(Object.keys(accounts)).toEqual(withFact((row) => row.savedAccounts));
  });
});
