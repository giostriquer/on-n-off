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
  ["claude", "Claude", "claude", "~/.claude/", "~/.claude.json", true],
  ["codex", "Codex", "codex", "~/.codex/", "~/.codex/config.toml", true],
  ["antigravity", "Antigravity", "agy", "~/.gemini/", "~/.gemini/config/mcp_config.json", true],
  ["cursor", "Cursor", "agent", "~/.cursor/", "~/.cursor/mcp.json", false],
  ["muse", "Muse", "muse", "~/.local/share/muse/", "~/.config/muse/settings.json", false],
] as const)("names %s's CLI, its home and MCP config, and whether on-n-off switches its MCP servers", (id, displayName, binary, home, mcpConfig, mcpToggle) => {
  render(<AgentConfig agent={{ ...claude, id, displayName }} tab={null} />);

  const paths = screen.getByRole("region", { name: "Config paths" });
  expect(within(within(paths).getByText("CLI binary").parentElement!).getByTitle(binary)).toBeInTheDocument();
  expect(within(within(paths).getByText("Global config").parentElement!).getByTitle(home)).toBeInTheDocument();
  expect(within(within(paths).getByText("MCP config").parentElement!).getByTitle(mcpConfig)).toBeInTheDocument();
  const capabilities = screen.getByRole("region", { name: "Adapter capabilities" });
  const toggle = within(capabilities).getByText("MCP toggle").parentElement!.parentElement!;
  expect(within(toggle).getByText(mcpToggle ? "YES" : "NO")).toBeInTheDocument();
});
