import { describe, expect, it } from "vitest";
import { agentInfo } from "@/dev/agentFixtures";
import { NOTHING_OFFERED } from "./capabilities";
import { PROVIDER_IDS } from "./providers";
import { LOCKED_AGENTS, keepShownPluginMetadata, mergeEnrichedPluginMetadata, overlayAgents } from "./session";
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

describe("keepShownPluginMetadata", () => {
  it("keeps the upstream the tab showed for a plugin whose installed version did not change", () => {
    const shown = tab({ upstream: "0.24.0", outOfSync: true });
    const answer = tab({ enabled: true });

    const kept = keepShownPluginMetadata(answer, shown);

    expect(kept.plugins[0]).toEqual({ ...answer.plugins[0], upstream: "0.24.0", outOfSync: true });
    expect(kept.userSkills).toBe(answer.userSkills);
    expect(kept.mcpServers).toBe(answer.mcpServers);
  });

  it("takes the answer's own metadata for a plugin the change moved to another version", () => {
    const shown = tab({ upstream: "0.24.0", outOfSync: true });
    const answer = tab({ version: "0.24.0", upstream: "0.24.0", outOfSync: false });

    expect(keepShownPluginMetadata(answer, shown).plugins[0]).toBe(answer.plugins[0]);
  });

  it("keeps or drops the shown metadata plugin by plugin, matching each by id and installed version", () => {
    const [plugin] = tab().plugins;
    const workbench = { ...plugin, id: "workbench@workshop", name: "workbench" };
    const toolkit = { ...plugin, id: "toolkit@workshop", name: "toolkit", skills: [] };
    const shown: AgentTabDto = {
      ...tab(),
      plugins: [
        { ...workbench, version: "0.22.0", upstream: "0.23.0", outOfSync: true },
        { ...toolkit, version: "0.22.0", upstream: "0.25.0", outOfSync: true },
      ],
    };
    const answer: AgentTabDto = {
      ...tab(),
      plugins: [
        { ...workbench, version: "0.23.0", upstream: "0.23.0", outOfSync: false },
        { ...toolkit, version: "0.22.0", upstream: "0.22.0", outOfSync: false, enabled: true },
      ],
    };

    const kept = keepShownPluginMetadata(answer, shown);

    expect(kept.plugins[0]).toBe(answer.plugins[0]);
    expect(kept.plugins[1]).toEqual({ ...answer.plugins[1], upstream: "0.25.0", outOfSync: true });
  });

  it("answers as is when nothing was shown", () => {
    const answer = tab();

    expect(keepShownPluginMetadata(answer, null)).toBe(answer);
  });
});

describe("overlayAgents", () => {
  it("keeps every field Rust sends for each provider, in tab order", () => {
    const sent = [agentInfo("muse"), agentInfo("claude", "missing")];
    const agents = overlayAgents(sent);
    expect(agents.map((agent) => agent.id)).toEqual(PROVIDER_IDS);
    expect(agents.find((agent) => agent.id === "muse")).toEqual(sent[0]);
    expect(agents.find((agent) => agent.id === "claude")).toEqual(sent[1]);
  });

  it("offers nothing for a provider Rust did not describe", () => {
    const codex = overlayAgents([agentInfo("muse")]).find((agent) => agent.id === "codex");
    expect(codex).toEqual({
      ...LOCKED_AGENTS.find((agent) => agent.id === "codex"),
      cliOk: false,
      cliError: "Agent health unavailable.",
    });
    expect(codex?.capabilities).toBe(NOTHING_OFFERED);
  });
});
