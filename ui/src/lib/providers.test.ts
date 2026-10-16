import { describe, expect, it } from "vitest";
import { PROVIDER_IDS, isAgentId, providerLabel, providerNames, providerRecord } from "./providers";

describe("providers", () => {
  it("labels every provider for display", () => {
    expect(PROVIDER_IDS.map(providerLabel)).toEqual(["Claude", "Codex", "Antigravity", "Cursor", "Muse"]);
  });

  it("knows a provider only by its exact key", () => {
    for (const id of PROVIDER_IDS) expect(isAgentId(id), id).toBe(true);
    for (const value of ["gemini", "Claude", "claude ", "", "toString", "__proto__", null, undefined, 1]) {
      expect(isAgentId(value), String(value)).toBe(false);
    }
  });

  it("names a list of providers the way a sentence would", () => {
    expect(providerNames([])).toBe("");
    expect(providerNames(["muse"])).toBe("Muse");
    expect(providerNames(["claude", "codex"])).toBe("Claude or Codex");
    expect(providerNames(["claude", "codex", "muse"])).toBe("Claude, Codex or Muse");
  });

  it("builds its own value for every provider, from that provider's key", () => {
    const record = providerRecord((id) => `${id}!`);
    expect(Object.keys(record)).toEqual(PROVIDER_IDS);
    for (const id of PROVIDER_IDS) expect(record[id]).toBe(`${id}!`);
    const lists = providerRecord(() => []);
    expect(lists.claude).not.toBe(lists.codex);
  });
});
