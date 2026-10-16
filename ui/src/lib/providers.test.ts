import { describe, expect, it } from "vitest";
import {
  PROVIDER_IDS,
  isAgentId,
  providerChartColor,
  providerColor,
  providerIconColor,
  providerLabel,
  providerNames,
  providerRecord,
} from "./providers";

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

  it.each([
    ["claude", "#d97757", "#d97757", "#d9773a", "#d97757"],
    ["codex", "var(--silkscreen)", "var(--silkscreen)", "#2a2a2a", "#ececec"],
    ["antigravity", "var(--mute)", "#3186ff", "#6c727c", "#8c939d"],
    ["cursor", "#7aa2ff", "var(--silkscreen)", "#3b6dff", "#7aa2ff"],
    ["muse", "#0082fb", "#0082fb", "#0082fb", "#0082fb"],
  ] as const)("paints %s's bars, icon and chart bands in light and dark", (id, bar, icon, light, dark) => {
    expect(providerColor(id)).toBe(bar);
    expect(providerIconColor(id)).toBe(icon);
    expect(providerChartColor(id, false)).toBe(light);
    expect(providerChartColor(id, true)).toBe(dark);
  });
});
