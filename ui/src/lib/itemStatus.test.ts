import { describe, expect, it } from "vitest";
import { itemBadges, statusForSkill, upstreamLabel } from "./itemStatus";
import type { ItemStatus, SkillDto } from "./types";

function status(overrides: Partial<ItemStatus>): ItemStatus {
  return {
    id: "claude:skill:/home/me/.claude/skills/tdd",
    provider: "claude",
    kind: "skill",
    name: "tdd",
    displayName: "tdd",
    targetPath: "/home/me/.claude/skills/tdd",
    installedVersion: "1.2.3",
    installedSha: "a".repeat(40),
    modified: false,
    missing: false,
    upstream: { state: "current" },
    source: { owner: "acme", repo: "skills", ref: "HEAD" },
    pluginName: "acme-skills",
    upstreamPath: "skills/ops/tdd",
    upstreamUrl: `https://github.com/acme/skills/tree/${"a".repeat(40)}/skills/ops/tdd`,
    ...overrides,
  };
}

function skill(overrides: Partial<SkillDto>): SkillDto {
  return {
    id: "user:tdd",
    pluginId: null,
    name: "tdd",
    description: "",
    enabled: true,
    togglable: true,
    origin: "user",
    ...overrides,
  };
}

describe("itemStatus", () => {
  it("matches user skills to global statuses and project skills to project statuses by name", () => {
    const global = [status({})];
    const project = [status({ id: "p", targetPath: "/proj/.claude/skills/tdd", installedVersion: "0.9.0" })];
    expect(statusForSkill(skill({}), { global, project })?.installedVersion).toBe("1.2.3");
    expect(
      statusForSkill(skill({ id: "project:tdd", origin: "project", togglable: false }), { global, project })
        ?.installedVersion,
    ).toBe("0.9.0");
    expect(statusForSkill(skill({ name: "other" }), { global, project })).toBeUndefined();
    // Plugin skills are never managed items.
    expect(statusForSkill(skill({ pluginId: "x@y" }), { global, project })).toBeUndefined();
    // A renamed local copy still matches through the recorded name.
    expect(statusForSkill(skill({ name: "my-tdd" }), { global: [status({ displayName: "my-tdd" })], project: [] })).toBeTruthy();
  });

  it("builds badges for version, update, local edits, and missing copies", () => {
    expect(itemBadges(status({})).map((b) => b.label)).toEqual(["v1.2.3"]);
    expect(
      itemBadges(
        status({
          modified: true,
          upstream: { state: "updateAvailable", commitSha: "b".repeat(40), pluginVersion: "1.3.0" },
        }),
      ).map((b) => `${b.tone}:${b.label}`),
    ).toEqual(["mute:v1.2.3", "warn:update available → v1.3.0", "warn:modified locally"]);
    expect(itemBadges(status({ missing: true, installedVersion: null })).map((b) => b.label)).toEqual([
      "missing on disk",
    ]);
    expect(upstreamLabel(status({ upstream: { state: "updateAvailable", commitSha: "b".repeat(40), pluginVersion: null } }))).toBe(
      "bbbbbbb",
    );
  });
});
