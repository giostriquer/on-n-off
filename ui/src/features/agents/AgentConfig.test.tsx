import { render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { mcpConfigPath } from "$lib/catalog";
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

// The row names Claude's MCP config file, so it counts the servers read from that file alone —
// not a plugin's, not one kept for particular projects, not the selected project's.
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

  const row = screen.getByTitle(mcpConfigPath("claude")).parentElement!;
  expect(within(row).getByText("2 FOUND")).toBeInTheDocument();
});
