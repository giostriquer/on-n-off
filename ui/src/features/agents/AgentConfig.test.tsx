import { render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { providerPaths } from "$lib/providers";
import type { AgentInfo, AgentTabDto, McpServerDto } from "$lib/types";
import { AgentConfig } from "./AgentConfig";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode; [key: string]: unknown }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

const claude: AgentInfo = {
  id: "claude",
  displayName: "Claude",
  cliOk: true,
  cliError: null,
  installGit: true,
  installFolder: false,
  pluginToggle: true,
  readsHooks: true,
};

const server = (id: string, origin: McpServerDto["origin"]): McpServerDto => ({
  id,
  name: id,
  system: "stdio",
  source: "run",
  enabled: true,
  togglable: origin === "",
  origin,
});

it("counts only the servers in the MCP config file beside its path", () => {
  const tab: AgentTabDto = {
    plugins: [],
    userSkills: [],
    mcpServers: [
      server("github", ""),
      server("docs", ""),
      server("plugin:kit:tracker", "plugin"),
      server("local:pad", "local"),
      server("project:repo-docs", "project"),
    ],
    hooks: [],
  };
  render(<AgentConfig agent={claude} tab={tab} />);

  const row = screen.getByTitle(providerPaths("claude").mcpConfig).parentElement!;
  expect(within(row).getByText("2 FOUND")).toBeInTheDocument();
});

it.each([
  {
    id: "claude", displayName: "Claude", binary: "claude", home: "~/.claude/",
    plugins: "~/.claude/plugins", skills: "~/.claude/plugins · user skills", mcpConfig: "~/.claude.json",
    pluginToggle: "claude plugin enable/disable", mcpToggle: "file patch · ~/.claude.json disabled", mcpSwitchable: true,
  },
  {
    id: "codex", displayName: "Codex", binary: "codex", home: "~/.codex/",
    plugins: "~/.codex/plugins", skills: "~/.codex/skills · ~/.agents/skills", mcpConfig: "~/.codex/config.toml",
    pluginToggle: "file patch · [plugins.*.enabled]", mcpToggle: "file patch · [mcp_servers.*.enabled]", mcpSwitchable: true,
  },
  {
    id: "antigravity", displayName: "Antigravity", binary: "agy", home: "~/.gemini/",
    plugins: "~/.gemini/config/plugins · antigravity-cli/plugins", skills: "~/.gemini/antigravity-cli/skills · .agents/skills",
    mcpConfig: "~/.gemini/config/mcp_config.json",
    pluginToggle: "agy plugin enable/disable", mcpToggle: "file patch · mcpServers.*.disabled", mcpSwitchable: true,
  },
  {
    id: "cursor", displayName: "Cursor", binary: "agent", home: "~/.cursor/",
    plugins: "~/.cursor/plugins", skills: "~/.cursor/skills · .cursor/skills", mcpConfig: "~/.cursor/mcp.json",
    pluginToggle: "not implemented yet", mcpToggle: "managed in Cursor · read-only here", mcpSwitchable: false,
  },
  {
    id: "muse", displayName: "Muse", binary: "muse", home: "~/.local/share/muse/",
    plugins: "~/.local/share/muse/plugins", skills: "~/.config/muse/skills · ~/.agents/skills", mcpConfig: "~/.config/muse/settings.json",
    pluginToggle: "muse plugins enable/disable", mcpToggle: "managed in Muse · read-only here", mcpSwitchable: false,
  },
] as const)("names $id's CLI, the paths it reads and how on-n-off switches its plugins and MCP servers", (provider) => {
  render(<AgentConfig agent={{ ...claude, id: provider.id, displayName: provider.displayName }} tab={null} />);

  const paths = screen.getByRole("region", { name: "Config paths" });
  const path = (label: string) => within(paths).getByText(label).parentElement!.children[1]!.textContent;
  expect(path("CLI binary")).toBe(provider.binary);
  expect(path("Global config")).toBe(provider.home);
  expect(path("Plugins dir")).toBe(provider.plugins);
  expect(path("Skills")).toBe(provider.skills);
  expect(path("MCP config")).toBe(provider.mcpConfig);
  expect(path("Backup dir")).toBe("~/.on-n-off/backups");
  const scope = screen.getByRole("region", { name: "Project scope" });
  expect(within(scope).getByText("Active scope").parentElement!.lastElementChild!.textContent).toBe(`${provider.home} · global source of truth`);
  expect(within(within(scope).getByRole("button", { name: /^All projects/ })).getByText(provider.home)).toBeInTheDocument();
  const capabilities = screen.getByRole("region", { name: "Adapter capabilities" });
  const capability = (label: string) => within(capabilities).getByText(label).parentElement!.parentElement!;
  expect(within(capability("Plugin toggle")).getByText(provider.pluginToggle)).toBeInTheDocument();
  expect(within(capability("Plugin toggle")).getByText("YES")).toBeInTheDocument();
  expect(within(capability("MCP toggle")).getByText(provider.mcpToggle)).toBeInTheDocument();
  expect(within(capability("MCP toggle")).getByText(provider.mcpSwitchable ? "YES" : "NO")).toBeInTheDocument();
});

it("shows the selected project's path as the active scope", () => {
  const project = { id: "/Users/me/acme", label: "acme", path: "/Users/me/acme", branch: "main", skillCount: 1, mcpCount: 0 };
  render(<AgentConfig agent={claude} tab={null} projects={[project]} selectedPath="/Users/me/acme" />);

  const scope = screen.getByRole("region", { name: "Project scope" });
  expect(within(scope).getByText("Active scope").parentElement!.lastElementChild!.textContent).toBe("/Users/me/acme");
});
