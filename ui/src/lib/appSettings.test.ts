import { describe, expect, it } from "vitest";
import { visibleAgentIds, setAgentHidden, ALL_AGENTS } from "./appSettings";

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
});
