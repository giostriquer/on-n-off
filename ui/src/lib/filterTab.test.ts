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
  // One token per searchable field, shared with nothing else, so a query can only match through
  // the field it is meant to: drop one from the haystack and exactly one assertion below fails.
  hooks: [
    {
      id: ":settings.json:pre_tool_use:0:0",
      event: "PreToolUse",
      matcher: "Bash",
      handler: "command",
      command: "~/.claude/hooks/guard.sh",
      source: "settings.json",
      pluginId: null,
      description: "",
      enabled: true,
    },
    {
      id: "acme-guardrails@webapp:hooks/hooks.json:post_tool_use:0:0",
      event: "PostToolUse",
      matcher: "Edit",
      handler: "mcp_tool",
      command: "acme-review · lint_diff",
      source: "acme-guardrails",
      pluginId: "acme-guardrails@webapp",
      description: "Lints every diff the agent produces.",
      enabled: true,
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

  it("finds a server by any project that keeps it, or by the plugin that brings it", () => {
    const withSources: AgentTabDto = {
      ...tab,
      mcpServers: [
        ...tab.mcpServers,
        {
          id: "local:library-docs",
          name: "library-docs",
          system: "http",
          source: "https://docs.example/mcp",
          enabled: true,
          togglable: false,
          origin: "local",
          projects: ["/Users/me/acme/webapp", "/Users/me/acme/api"],
        },
        {
          id: "plugin:kit:tracker",
          name: "tracker",
          system: "http",
          source: "https://tracker.example/mcp",
          enabled: true,
          togglable: false,
          origin: "plugin",
          pluginId: "kit@marketplace-one",
        },
      ],
    };
    const ids = (query: string) => filterTab(withSources, query).mcpServers.map((server) => server.id);
    expect(ids("webapp")).toEqual(["local:library-docs"]);
    expect(ids("acme/api")).toEqual(["local:library-docs"]);
    expect(ids("marketplace-one")).toEqual(["plugin:kit:tracker"]);
    expect(ids("no-such-project")).toEqual([]);
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

describe("filterTab hooks", () => {
  const ids = (query: string) => filterTab(tab, query).hooks.map((entry) => entry.id);
  const plugin = "acme-guardrails@webapp:hooks/hooks.json:post_tool_use:0:0";
  const settings = ":settings.json:pre_tool_use:0:0";

  it("returns every hook, in the backend's order, when the query is empty", () => {
    expect(ids("  ")).toEqual([plugin, settings]);
  });

  it("matches event, matcher, handler, command, source, description and plugin id", () => {
    expect(ids("posttooluse")).toEqual([plugin]);
    expect(ids("edit")).toEqual([plugin]);
    expect(ids("mcp_tool")).toEqual([plugin]);
    expect(ids("lint_diff")).toEqual([plugin]);
    expect(ids("webapp")).toEqual([plugin]);
    expect(ids("produces")).toEqual([plugin]);
    expect(ids("settings.json")).toEqual([settings]);
    expect(ids("bash")).toEqual([settings]);
    expect(ids("guard.sh")).toEqual([settings]);
    expect(ids("nothing here")).toEqual([]);
  });

  it("leaves the synthetic id out of the haystack", () => {
    // `…:<event>:<group>:<index>` is a key, not something a user types: searching its digits or
    // its snake_cased event would otherwise match rows that show neither.
    expect(ids("0")).toEqual([]);
    expect(ids("pre_tool_use")).toEqual([]);
  });
});

describe("filterMcpList", () => {
  it("matches mcp name, transport, and source", () => {
    expect(filterMcpList(tab, "").map((server) => server.id)).toEqual(["github"]);
    expect(filterMcpList(tab, "stdio").map((server) => server.id)).toEqual(["github"]);
    expect(filterMcpList(tab, "nope")).toEqual([]);
  });
});
