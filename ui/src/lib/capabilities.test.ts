import { describe, expect, it } from "vitest";
import rustAgents from "../../../src-tauri/src/capabilities/fixtures/agents.json";
import { agentInfo, declaredCapabilities } from "@/dev/agentFixtures";
import {
  canInstall,
  canSwitchPlugins,
  canUninstallPlugin,
  canUpdate,
  chosenAgents,
  holdsSubagents,
  installsFrom,
  mcpNotice,
  previewPath,
  takesSubagents,
} from "./capabilities";
import { PROVIDER_IDS } from "./providers";
import { LOCKED_AGENTS, overlayAgents } from "./session";
import type { AgentInfo, Change, InstallSourceKind, PluginDto } from "./types";

const CHANGES = ["pluginSwitch", "skillSwitch", "mcpSwitch", "install", "uninstall", "update"] as const;
const SOURCE_KINDS: readonly string[] = [
  "gitMarketplace",
  "folder",
  "marketplacePlugin",
  "marketplaceSkill",
] satisfies InstallSourceKind[];

function isChange(value: unknown): value is Change {
  if (typeof value !== "object" || value === null) return false;
  const change = value as Record<string, unknown>;
  if (typeof change.how !== "string") return false;
  if (change.by === "cli" || change.by === "file") return Object.keys(change).length === 2;
  return change.by === "refused" && typeof change.reason === "string" && change.reason.length > 0;
}

const plugin = (source: string, id = `acme@${source}`): PluginDto => ({
  id,
  name: "acme",
  source,
  version: "",
  upstream: "",
  enabled: true,
  togglable: true,
  skills: [],
});

describe("what each adapter declares it can change", () => {
  it("reaches the UI in the shape its types name", () => {
    expect(Object.keys(rustAgents).sort()).toEqual([...PROVIDER_IDS].sort());
    for (const id of PROVIDER_IDS) {
      const declared = declaredCapabilities(id);
      for (const change of CHANGES) expect(isChange(declared[change]), `${id} ${change}`).toBe(true);
      for (const kind of Object.keys(declared.refusedSources)) expect(SOURCE_KINDS, `${id} ${kind}`).toContain(kind);
    }
  });

  it.each(PROVIDER_IDS.flatMap((id) => (["found", "missing"] as const).map((cli) => [id, cli] as const)))(
    "says what %s can do with its CLI %s exactly as Rust works it out",
    (id, cli) => {
      const agent = agentInfo(id, cli);
      expect(canSwitchPlugins(agent)).toBe(agent.pluginToggle);
      expect(installsFrom(agent, "gitMarketplace")).toBe(agent.installGit);
      expect(installsFrom(agent, "folder")).toBe(agent.installFolder);
      expect(agent.capabilities.readsHooks).toBe(agent.readsHooks);
    },
  );

  it("offers Uninstall only where the adapter uninstalls, and never for a project's own plugin", () => {
    const claude = agentInfo("claude");
    const antigravity = agentInfo("antigravity");
    expect(canUninstallPlugin(claude, plugin("workshop"))).toBe(true);
    expect(canUninstallPlugin(agentInfo("claude", "missing"), plugin("workshop"))).toBe(false);
    expect(canUninstallPlugin(antigravity, plugin("cli"))).toBe(true);
    expect(canUninstallPlugin(antigravity, plugin("config"))).toBe(false);
    expect(canUninstallPlugin(antigravity, plugin("workspace", "project:ws@workspace"))).toBe(false);
    expect(canUninstallPlugin(claude, plugin("webapp", "project:acme@webapp"))).toBe(false);
    expect(canUninstallPlugin(agentInfo("cursor"), plugin("local"))).toBe(false);
  });

  it("offers the install sheet while the adapter installs from some source and its CLI is found", () => {
    expect(canInstall(agentInfo("muse"))).toBe(true);
    expect(canInstall(agentInfo("claude"))).toBe(true);
    expect(canInstall(agentInfo("muse", "missing"))).toBe(false);
    expect(canInstall(agentInfo("cursor"))).toBe(false);
    const refusesAll = agentInfo("muse");
    refusesAll.capabilities = {
      ...refusesAll.capabilities,
      refusedSources: { gitMarketplace: "no", folder: "no", marketplacePlugin: "no", marketplaceSkill: "no" },
    };
    expect(canInstall(refusesAll)).toBe(false);
  });

  it("offers Update only where the adapter updates and its CLI is found", () => {
    expect(canUpdate(agentInfo("claude"))).toBe(true);
    expect(canUpdate(agentInfo("muse"))).toBe(true);
    expect(canUpdate(agentInfo("claude", "missing"))).toBe(false);
    expect(canUpdate(agentInfo("antigravity"))).toBe(false);
    expect(canUpdate(agentInfo("cursor"))).toBe(false);
  });

  it("shows the reason an MCP server cannot be switched as the MCP screen's notice", () => {
    for (const id of PROVIDER_IDS) {
      const agent = agentInfo(id);
      const change = agent.capabilities.mcpSwitch;
      expect(mcpNotice(agent), id).toBe(change.by === "refused" ? change.reason : null);
    }
    expect(mcpNotice(agentInfo("cursor"))).toMatch(/^Cursor keeps MCP servers on or off in its own settings/);
    expect(mcpNotice(agentInfo("claude"))).toBeNull();
  });

  it("lets subagents in only when a chosen provider has a place for them in that scope", () => {
    const agents: AgentInfo[] = PROVIDER_IDS.map((id) => agentInfo(id));
    const global = { kind: "global" } as const;
    const project = { kind: "project", projectPath: "/Users/me/webapp" } as const;
    expect(takesSubagents(agents, ["codex"], global)).toBe(false);
    expect(takesSubagents(agents, ["codex", "claude"], global)).toBe(true);
    expect(takesSubagents(agents, ["codex", "claude"], project)).toBe(true);
    expect(takesSubagents(agents.filter((agent) => agent.id !== "claude"), ["claude"], global)).toBe(false);
    const claude = agentInfo("claude");
    const projectOnly = { ...claude, itemRoots: { global: claude.itemRoots.global && { ...claude.itemRoots.global, agents: null }, project: claude.itemRoots.project } };
    expect(takesSubagents([projectOnly], ["claude"], global)).toBe(false);
    expect(takesSubagents([projectOnly], ["claude"], project)).toBe(true);
  });

  it("shows installed subagents only for an agent with a place for them", () => {
    expect(holdsSubagents(agentInfo("claude"))).toBe(true);
    for (const id of ["codex", "antigravity", "cursor", "muse"] as const) expect(holdsSubagents(agentInfo(id)), id).toBe(false);
    expect(holdsSubagents({ ...agentInfo("claude"), itemRoots: { global: null, project: null } })).toBe(false);
    const claude = agentInfo("claude");
    const inProjectsOnly = { ...claude, itemRoots: { global: { skills: "~/.claude/skills", agents: null }, project: claude.itemRoots.project } };
    expect(holdsSubagents(inProjectsOnly)).toBe(true);
  });

  it("lists the chosen agents in the order they were chosen", () => {
    const agents: AgentInfo[] = PROVIDER_IDS.map((id) => agentInfo(id));
    expect(chosenAgents(agents, ["muse", "claude"]).map((agent) => agent.id)).toEqual(["muse", "claude"]);
    expect(chosenAgents(agents.slice(1), ["claude", "codex"]).map((agent) => agent.id)).toEqual(["codex"]);
  });

  it("switches plugins and installs from a source only as the declaration and the CLI allow", () => {
    expect(canSwitchPlugins(agentInfo("codex", "missing"))).toBe(true);
    expect(canSwitchPlugins(agentInfo("claude", "missing"))).toBe(false);
    expect(canSwitchPlugins(agentInfo("cursor"))).toBe(false);
    expect(installsFrom(agentInfo("muse"), "folder")).toBe(true);
    expect(installsFrom(agentInfo("muse"), "gitMarketplace")).toBe(false);
    expect(installsFrom(agentInfo("muse", "missing"), "folder")).toBe(false);
    expect(installsFrom(agentInfo("claude"), "gitMarketplace")).toBe(true);
  });

  it("previews where an item goes from the agent's own item roots", () => {
    const claude = agentInfo("claude");
    expect(previewPath(claude, { kind: "global" })).toBe(claude.itemRoots.global?.skills);
    expect(previewPath(claude, { kind: "project", projectPath: "/Users/me/webapp/" })).toBe(
      "/Users/me/webapp/.claude/skills",
    );
    expect(previewPath(agentInfo("codex"), { kind: "project", projectPath: "E:/dev/app" })).toBe(
      "E:/dev/app/.codex/skills",
    );
    expect(previewPath(claude, { kind: "project", projectPath: "/Users/me/webapp//" })).toBe(
      "/Users/me/webapp/.claude/skills",
    );
    expect(previewPath({ ...claude, itemRoots: { global: null, project: null } }, { kind: "global" })).toBe("");
  });
});

describe("an agent Rust has not described", () => {
  it.each([
    ["before list_agents answers", LOCKED_AGENTS[0]],
    ["when list_agents fails", overlayAgents([])[0]],
    ["even with its CLI found", { ...LOCKED_AGENTS[0], cliOk: true }],
  ])("offers nothing %s", (_when, agent) => {
    const global = { kind: "global" } as const;
    expect(agent.itemRoots).toEqual({ global: null, project: null });
    expect(mcpNotice(agent)).toBeNull();
    expect(agent.capabilities.readsHooks).toBe(false);
    expect(canInstall(agent)).toBe(false);
    expect(canSwitchPlugins(agent)).toBe(false);
    expect(canUpdate(agent)).toBe(false);
    expect(canUninstallPlugin(agent, plugin("webapp"))).toBe(false);
    expect(takesSubagents([agent], [agent.id], global)).toBe(false);
    expect(holdsSubagents(agent)).toBe(false);
    expect(previewPath(agent, global)).toBe("");
  });
});
