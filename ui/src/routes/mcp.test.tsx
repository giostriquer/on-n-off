import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { McpRoute } from "./mcp";
import { emptyTabDto } from "$lib/catalog";
import type { AgentId } from "$lib/types";

const session = vi.hoisted(() => ({ provider: "claude" as AgentId }));

vi.mock("@/features/session/SessionProvider", () => ({
  useAgentSession: () => ({
    currentAgent: { id: session.provider },
    currentTab: { dto: emptyTabDto(), filter: "", inFlight: false, loading: false, error: null },
    filtered: null,
    emptyTabDto,
    toggleMcp: () => undefined,
  }),
}));

describe("McpRoute", () => {
  it.each([
    ["claude", null],
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
  ] as const)("says on %s's screen whether on-n-off can switch its MCP servers", (provider, notice) => {
    session.provider = provider;
    render(<McpRoute />);

    if (notice) expect(screen.getByRole("note").textContent).toBe(notice);
    else expect(screen.queryByRole("note")).toBeNull();
  });
});
