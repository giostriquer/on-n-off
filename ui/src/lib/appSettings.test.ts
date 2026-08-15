import { describe, expect, it } from "vitest";
import { visibleAgentIds, setAgentHidden, ALL_AGENTS, mergeAppSettings } from "./appSettings";

describe("appSettings", () => {
  it("defaults to every provider visible", () => {
    expect(visibleAgentIds([])).toEqual([...ALL_AGENTS]);
  });

  it("hides a provider from tabs without emptying the list", () => {
    expect(visibleAgentIds(["antigravity"])).toEqual(["claude", "codex"]);
  });

  it("refuses to hide the last visible provider", () => {
    const hidden = setAgentHidden(["codex", "antigravity"], "claude", true);
    expect(hidden).toEqual(["codex", "antigravity"]);
    expect(visibleAgentIds(hidden)).toEqual(["claude"]);
  });

  it("can show a hidden provider again", () => {
    expect(setAgentHidden(["antigravity"], "antigravity", false)).toEqual([]);
  });

  it("enables automatic updates by default and preserves an opt-out", () => {
    expect((mergeAppSettings(null) as unknown as Record<string, unknown>).automaticUpdates).toBe(true);
    expect(
      (mergeAppSettings(JSON.parse('{"automaticUpdates":false}')) as unknown as Record<string, unknown>)
        .automaticUpdates,
    ).toBe(false);
  });
});
