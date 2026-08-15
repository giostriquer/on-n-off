import { describe, expect, it } from "vitest";
import { filterMcpList, filterSkillList, filterTab } from "./filterTab";
import type { AgentTabDto } from "./types";

const tab: AgentTabDto = {
  plugins: [
    {
      id: "workbench@workshop",
      name: "workbench",
      source: "workshop",
      version: "0.22.1",
      upstream: "0.23.0",
      enabled: true,
      togglable: true,
      skills: [
        {
          id: "workbench@workshop:brainstorming",
          pluginId: "workbench@workshop",
          name: "brainstorming",
          description: "Turn ideas into designs",
          enabled: true,
          togglable: false,
        },
      ],
    },
  ],
  userSkills: [
    {
      id: "statusline",
      pluginId: null,
      name: "statusline",
      description: "Custom status line",
      enabled: true,
      togglable: true,
    },
  ],
  mcpServers: [
    {
      id: "github",
      name: "github",
      system: "stdio",
      source: "npx -y @modelcontextprotocol/server-github",
      enabled: true,
      togglable: true,
    },
  ],
};

describe("filterTab", () => {
  it("returns the full tab when the query is empty", () => {
    const filtered = filterTab(tab, "  ");
    expect(filtered.plugins.map((plugin) => plugin.id)).toEqual(["workbench@workshop"]);
    expect(filtered.skills.map((skill) => skill.name)).toEqual(["statusline", "brainstorming"]);
    expect(filtered.mcpServers.map((server) => server.id)).toEqual(["github"]);
  });

  it("expands a plugin when a nested skill matches", () => {
    const filtered = filterTab(tab, "brainstorm");
    expect(filtered.plugins.map((plugin) => plugin.id)).toEqual(["workbench@workshop"]);
    expect(filtered.expandIds).toEqual(["workbench@workshop"]);
  });

  it("matches installed plugin version", () => {
    expect(filterTab(tab, "0.22.1").plugins.map((plugin) => plugin.id)).toEqual(["workbench@workshop"]);
    expect(filterTab(tab, "0.23.0").plugins.map((plugin) => plugin.id)).toEqual(["workbench@workshop"]);
  });
});

describe("filterSkillList", () => {
  it("flattens plugin and user skills", () => {
    expect(filterSkillList(tab, "").map((skill) => skill.name)).toEqual(["statusline", "brainstorming"]);
  });

  it("matches plugin id, skill name, and description", () => {
    expect(filterSkillList(tab, "workshop").map((skill) => skill.name)).toEqual(["brainstorming"]);
    expect(filterSkillList(tab, "status").map((skill) => skill.name)).toEqual(["statusline"]);
    expect(filterSkillList(tab, "designs").map((skill) => skill.name)).toEqual(["brainstorming"]);
  });
});

describe("filterMcpList", () => {
  it("matches mcp name, transport, and source", () => {
    expect(filterMcpList(tab, "").map((server) => server.id)).toEqual(["github"]);
    expect(filterMcpList(tab, "stdio").map((server) => server.id)).toEqual(["github"]);
    expect(filterMcpList(tab, "nope")).toEqual([]);
  });
});
