import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { McpRoute } from "./mcp";
import { agentInfo } from "@/dev/agentFixtures";
import { emptyTabDto } from "$lib/catalog";
import type { AgentId } from "$lib/types";

const session = vi.hoisted(() => ({ provider: "claude" as AgentId, scope: null as string | null }));

vi.mock("@/features/session/SessionProvider", () => ({
  useAgentSession: () => ({
    currentAgent: agentInfo(session.provider),
    currentScopePath: session.scope,
    currentTab: { dto: emptyTabDto(), filter: "", inFlight: false, loading: false, error: null },
    filtered: null,
    emptyTabDto,
    toggleMcp: () => undefined,
  }),
}));

const CLAUDE_IN_PROJECTS =
  "Claude Code turns MCP servers off one project at a time, so on-n-off switches them only inside a project. Choose a project under Scope, or use /mcp in Claude Code.";

describe("McpRoute", () => {
  it.each([
    ["claude", CLAUDE_IN_PROJECTS],
    ["codex", null],
    ["antigravity", null],
    [
      "cursor",
      "Cursor keeps MCP servers on or off in its own settings, so on-n-off lists them but cannot switch them here. Manage them in the Cursor app, or run `agent mcp enable <name>` / `agent mcp disable <name>`.",
    ],
    [
      "muse",
      "Muse manages MCP servers itself, so on-n-off lists them but cannot switch them here. Edit `mcpServers` in Muse’s settings.json, or use `muse mcp login <name>`.",
    ],
  ] as const)("says on %s's screen for all projects whether on-n-off can switch its MCP servers", (provider, notice) => {
    session.provider = provider;
    session.scope = null;
    render(<McpRoute />);

    if (notice) expect(screen.getByRole("note").textContent).toBe(notice);
    else expect(screen.queryByRole("note")).toBeNull();
  });

  it.each([
    ["claude", null],
    ["codex", null],
    ["cursor", "Cursor keeps MCP servers on or off in its own settings"],
  ] as const)("says on %s's screen inside a project whether on-n-off can switch its MCP servers there", (provider, notice) => {
    session.provider = provider;
    session.scope = "/Users/me/acme/webapp";
    render(<McpRoute />);

    if (notice) expect(screen.getByRole("note").textContent).toMatch(new RegExp(`^${notice}`));
    else expect(screen.queryByRole("note")).toBeNull();
  });
});
