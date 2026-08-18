import { describe, expect, it } from "vitest";
import { agentsAllowed, allKeys, entryKey, previewPath, selectedItems, toggleGroup } from "./marketplaceSelection";
import type { MarketplaceInspect } from "./types";

const inspect: MarketplaceInspect = {
  isMarketplace: true,
  commitSha: "a".repeat(40),
  marketplaceName: "mattpocock",
  hint: null,
  plugins: [
    {
      name: "mattpocock-skills",
      version: "1.2.3",
      description: "Matt's skills",
      supported: true,
      source: null,
      skills: [
        { name: "tdd", description: "Test-driven development", path: "skills/engineering/tdd" },
        { name: "grilling", description: "Ask hard questions", path: "skills/productivity/grilling" },
      ],
      agents: [{ name: "reviewer", description: "Reviews code", path: "agents/reviewer.md" }],
    },
    {
      name: "remote",
      version: null,
      description: "",
      supported: false,
      source: null,
      skills: [],
      agents: [],
    },
    {
      name: "gh",
      version: null,
      description: "",
      supported: true,
      source: { owner: "other", repo: "repo", ref: "HEAD" },
      skills: [{ name: "gamma", description: "", path: "skills/gamma" }],
      agents: [],
    },
  ],
};

describe("marketplaceSelection", () => {
  it("lists every supported entry and maps keys back to picks with their source", () => {
    const keys = allKeys(inspect);
    expect(keys).toHaveLength(4);
    const picks = selectedItems(inspect, new Set(keys));
    expect(picks).toEqual([
      { pluginName: "mattpocock-skills", kind: "skill", path: "skills/engineering/tdd", source: null },
      { pluginName: "mattpocock-skills", kind: "skill", path: "skills/productivity/grilling", source: null },
      { pluginName: "mattpocock-skills", kind: "agent", path: "agents/reviewer.md", source: null },
      { pluginName: "gh", kind: "skill", path: "skills/gamma", source: { owner: "other", repo: "repo", ref: "HEAD" } },
    ]);
  });

  it("toggles a plugin group on and off without touching other groups", () => {
    const one = new Set([entryKey("gh", "skill", "skills/gamma")]);
    const matt = inspect.plugins[0];
    const on = toggleGroup(one, matt, "skill", true);
    expect(on.size).toBe(3);
    const off = toggleGroup(on, matt, "skill", false);
    expect([...off]).toEqual([entryKey("gh", "skill", "skills/gamma")]);
  });

  it("only allows agents when Claude is a target and previews provider paths", () => {
    expect(agentsAllowed(["codex"])).toBe(false);
    expect(agentsAllowed(["codex", "claude"])).toBe(true);
    expect(previewPath("claude", { kind: "global" })).toBe("~/.claude/skills");
    expect(previewPath("antigravity", { kind: "global" })).toBe("~/.gemini/antigravity-cli/skills");
    expect(previewPath("codex", { kind: "project", projectPath: "E:/dev/app" })).toBe("E:/dev/app/.codex/skills");
    expect(previewPath("cursor", { kind: "project", projectPath: "/home/me/app" })).toBe(
      "/home/me/app/.cursor/skills",
    );
  });
});
