import { describe, expect, it } from "vitest";
import {
  catalogCounts,
  canUninstallPlugin,
  comparePluginThenName,
  driftRows,
  formatPluginVersion,
  globalItemCount,
  liveRows,
  masterAllOn,
  pluginOutOfSync,
  pluginVersionNote,
  skillIsLive,
  sortPlugins,
  sortSkills,
  tallyLine,
} from "./catalog";
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
    {
      id: "superpowers@official",
      name: "superpowers",
      source: "official",
      version: "",
      upstream: "",
      enabled: false,
      togglable: true,
      skills: [
        {
          id: "superpowers@official:debug",
          pluginId: "superpowers@official",
          name: "debug",
          description: "Debug",
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
    {
      id: "loom-feed",
      pluginId: null,
      name: "loom-feed",
      description: "Feed Loom",
      enabled: false,
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
    {
      id: "docs",
      name: "docs",
      system: "http",
      source: "https://docs.example/mcp",
      enabled: false,
      togglable: true,
    },
  ],
};

describe("catalog", () => {
  it("counts plugins and all skills, including locked plugin skills", () => {
    const counts = catalogCounts(tab);
    expect(counts.plugins).toEqual({ on: 1, total: 2 });
    expect(counts.skills).toEqual({ on: 2, total: 4 });
    expect(counts.mcp).toEqual({ on: 1, total: 2 });
    expect(tallyLine(counts, "Claude")).toBe("1 plugins · 2 skills · 1 mcps live on Claude");
  });

  it("treats locked plugin skills as live only when the plugin is on", () => {
    expect(skillIsLive(tab.plugins[0].skills[0], tab)).toBe(true);
    expect(skillIsLive(tab.plugins[1].skills[0], tab)).toBe(false);
  });

  it("lists live rows sorted by plugin name then name", () => {
    const rows = liveRows(tab);
    expect(rows.map((row) => row.name)).toEqual(["github", "statusline", "brainstorming", "workbench"]);
    expect(rows.find((row) => row.id === "workbench@workshop")?.meta).toBe("plugin · workshop · 0.22.1");
  });

  it("flags catalog drift only when both versions exist and differ", () => {
    expect(pluginOutOfSync(tab.plugins[0])).toBe(true);
    expect(pluginOutOfSync(tab.plugins[1])).toBe(false);
    expect(driftRows(tab).map((row) => row.id)).toEqual(["workbench@workshop"]);
    expect(pluginVersionNote(tab.plugins[0])).toBe("upstream v0.23.0");
    expect(pluginVersionNote({ ...tab.plugins[0], upstream: "0.22.1" })).toBe("up to date");
    expect(pluginVersionNote({ ...tab.plugins[1], version: "1.0.0" })).toBe("installed");
    expect(formatPluginVersion("0.22.1")).toBe("v0.22.1");
    expect(formatPluginVersion("bd2122cb")).toBe("bd2122cb");
    expect(
      pluginOutOfSync({
        ...tab.plugins[0],
        version: "6.3.0",
        upstream: "b36e082",
      }),
    ).toBe(false);
    expect(
      pluginVersionNote({
        ...tab.plugins[0],
        version: "6.3.0",
        upstream: "b36e082",
      }),
    ).toBe("installed");
  });

  it("sorts skills by plugin name then skill name", () => {
    expect(sortSkills([...tab.plugins.flatMap((plugin) => plugin.skills), ...tab.userSkills], tab.plugins).map((skill) => skill.name)).toEqual([
      "loom-feed",
      "statusline",
      "debug",
      "brainstorming",
    ]);
  });

  it("compares plugin name then name case-insensitively", () => {
    expect(comparePluginThenName("Apple", "zeta", "apple", "alpha")).toBeGreaterThan(0);
    expect(comparePluginThenName("zebra", "alpha", "apple", "zeta")).toBeGreaterThan(0);
  });

  it("sorts plugins by marketplace then name", () => {
    expect(
      sortPlugins([
        { ...tab.plugins[0], name: "workbench", source: "workshop" },
        { ...tab.plugins[1], name: "warp", source: "claude-code-warp", skills: [] },
        { ...tab.plugins[0], id: "toolkit@workshop", name: "toolkit", source: "workshop", skills: [] },
        {
          ...tab.plugins[1],
          id: "frontend-design@claude-plugins-official",
          name: "frontend-design",
          source: "claude-plugins-official",
          skills: [],
        },
      ]).map((plugin) => plugin.name),
    ).toEqual(["warp", "frontend-design", "toolkit", "workbench"]);
  });

  it("treats project skills as live and not togglable", () => {
    const scoped: AgentTabDto = {
      ...tab,
      userSkills: [
        ...tab.userSkills,
        {
          id: "project:local-feed",
          pluginId: null,
          name: "local-feed",
          description: "Project only",
          enabled: true,
          togglable: false,
          origin: "project",
        },
      ],
    };
    expect(skillIsLive(scoped.userSkills[2], scoped)).toBe(true);
    expect(liveRows(scoped).find((row) => row.name === "local-feed")).toEqual({
      kind: "skill",
      id: "project:local-feed",
      name: "local-feed",
      meta: "skill · project",
      enabled: true,
      togglable: false,
    });
    expect(
      masterAllOn({
        ...scoped,
        plugins: scoped.plugins.map((plugin) => ({ ...plugin, enabled: true })),
        userSkills: scoped.userSkills.map((skill) => ({ ...skill, enabled: true })),
        mcpServers: [
          ...scoped.mcpServers.map((server) => ({ ...server, enabled: true })),
          {
            id: "project:repo-docs",
            name: "repo-docs",
            system: "stdio",
            source: "node docs.js",
            enabled: true,
            togglable: false,
            origin: "project",
          },
        ],
      }),
    ).toBe(true);
  });

  it("masterAllOn ignores non-togglable plugins", () => {
    expect(
      masterAllOn({
        ...tab,
        plugins: [
          { ...tab.plugins[0], enabled: true, togglable: true },
          { ...tab.plugins[1], enabled: false, togglable: false, source: "config" },
        ],
        userSkills: tab.userSkills.map((skill) => ({ ...skill, enabled: true })),
        mcpServers: tab.mcpServers.map((server) => ({ ...server, enabled: true })),
      }),
    ).toBe(true);
  });

  it("blocks uninstall for config and project plugins", () => {
    expect(canUninstallPlugin({ ...tab.plugins[0], source: "workshop" })).toBe(true);
    expect(canUninstallPlugin({ ...tab.plugins[0], source: "config" })).toBe(false);
    expect(canUninstallPlugin({ ...tab.plugins[0], id: "project:ws@workspace", source: "workspace" })).toBe(
      false,
    );
  });

  it("counts only global items for the scope picker", () => {
    expect(globalItemCount(tab)).toBe(2 + 2 + 2);
    expect(
      globalItemCount({
        ...tab,
        userSkills: [
          ...tab.userSkills,
          {
            id: "project:local-feed",
            pluginId: null,
            name: "local-feed",
            description: "local",
            enabled: true,
            togglable: false,
            origin: "project",
          },
        ],
        mcpServers: [
          ...tab.mcpServers,
          {
            id: "project:repo-docs",
            name: "repo-docs",
            system: "stdio",
            source: "node",
            enabled: true,
            togglable: false,
            origin: "project",
          },
        ],
      }),
    ).toBe(2 + 2 + 2);
  });

  it("reports master cut off when anything togglable is off", () => {
    expect(masterAllOn(tab)).toBe(false);
    expect(
      masterAllOn({
        ...tab,
        plugins: tab.plugins.map((plugin) => ({ ...plugin, enabled: true })),
        userSkills: tab.userSkills.map((skill) => ({ ...skill, enabled: true })),
        mcpServers: tab.mcpServers.map((server) => ({ ...server, enabled: true })),
      }),
    ).toBe(true);
  });
});
