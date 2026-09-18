import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HooksRoute } from "./hooks";
import { emptyTabDto } from "$lib/catalog";
import type { AgentId, HookDto } from "$lib/types";

const session = vi.hoisted(() => ({ provider: "claude" as AgentId, displayName: "Claude", filter: "" }));

const hook: HookDto = {
  id: "claude:acme-guardrails:PreToolUse:0:0",
  event: "PreToolUse",
  matcher: "Bash",
  handler: "command",
  command: "${CLAUDE_PLUGIN_ROOT}/bin/guard.sh",
  source: "acme-guardrails",
  pluginId: "acme-guardrails@webapp",
  description: "Refuses shell commands outside the workspace.",
  enabled: true,
};

vi.mock("@/features/session/SessionProvider", () => ({
  useAgentSession: () => ({
    currentAgent: { id: session.provider, displayName: session.displayName },
    currentTab: { dto: { ...emptyTabDto(), hooks: [hook] }, filter: session.filter, inFlight: false, loading: false, error: null },
    emptyTabDto,
  }),
}));

describe("HooksRoute", () => {
  it("lists the provider's hooks, filtered by the shell's filter box", () => {
    session.provider = "claude";
    session.filter = "";
    const { rerender } = render(<HooksRoute />);
    expect(screen.getByText("PreToolUse")).toBeInTheDocument();

    session.filter = "postoolu";
    rerender(<HooksRoute />);
    expect(screen.queryByText("PreToolUse")).toBeNull();
    expect(screen.getByText(/Nothing matches/)).toBeInTheDocument();
  });

  it("says so for a provider whose hooks on-n-off does not read", () => {
    session.provider = "cursor";
    session.displayName = "Cursor";
    session.filter = "";
    render(<HooksRoute />);

    expect(screen.getByText(/doesn’t read hooks for Cursor/)).toBeInTheDocument();
    expect(screen.queryByText("PreToolUse")).toBeNull();
  });
});
