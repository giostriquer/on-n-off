import { describe, expect, it } from "vitest";
import { mergeEnrichedPluginMetadata } from "./session";
import type { AgentTabDto } from "./types";

function tab(overrides: Partial<AgentTabDto["plugins"][number]> = {}): AgentTabDto {
  return {
    plugins: [
      {
        id: "workbench@workshop",
        name: "workbench",
        source: "workshop",
        version: "0.22.0",
        upstream: "0.22.0",
        outOfSync: false,
        enabled: false,
        togglable: true,
        skills: [
          {
            id: "brainstorming",
            pluginId: "workbench@workshop",
            name: "brainstorming",
            description: "local skill",
            enabled: false,
            togglable: false,
          },
        ],
        ...overrides,
      },
    ],
    userSkills: [
      {
        id: "local-user-skill",
        pluginId: null,
        name: "local-user-skill",
        description: "local user skill",
        enabled: true,
        togglable: true,
      },
    ],
    mcpServers: [
      {
        id: "local-mcp",
        name: "local-mcp",
        system: "stdio",
        source: "local",
        enabled: false,
        togglable: true,
      },
    ],
  };
}

describe("mergeEnrichedPluginMetadata", () => {
  it("changes only version, upstream, and drift fields for matching local plugins", () => {
    const local = tab();
    const remote = tab({
      version: "0.23.0",
      upstream: "0.24.0",
      outOfSync: true,
      enabled: true,
      togglable: false,
      skills: [],
    });
    remote.userSkills = [];
    remote.mcpServers = [];

    const merged = mergeEnrichedPluginMetadata(local, remote);

    expect(merged.plugins[0]).toEqual({
      ...local.plugins[0],
      version: "0.23.0",
      upstream: "0.24.0",
      outOfSync: true,
    });
    expect(merged.userSkills).toBe(local.userSkills);
    expect(merged.mcpServers).toBe(local.mcpServers);
  });

  it("leaves local plugins untouched when enrichment does not contain their id", () => {
    const local = tab();
    const remote: AgentTabDto = { plugins: [], userSkills: [], mcpServers: [] };

    const merged = mergeEnrichedPluginMetadata(local, remote);

    expect(merged.plugins[0]).toBe(local.plugins[0]);
  });
});
