import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HooksRoute } from "./hooks";
import { emptyTabDto } from "$lib/catalog";
import { filterTab } from "$lib/filterTab";
import type { AgentId, HookDto } from "$lib/types";

const hook: HookDto = {
  id: "acme-guardrails@webapp:hooks/hooks.json:pre_tool_use:0:0",
  event: "PreToolUse",
  matcher: "Bash",
  handler: "command",
  command: "${CLAUDE_PLUGIN_ROOT}/bin/guard.sh",
  source: "acme-guardrails",
  pluginId: "acme-guardrails@webapp",
  description: "Refuses shell commands outside the workspace.",
  enabled: true,
};

/** Mutated per test, so every field it holds is reset in `beforeEach` before the next one reads it. */
const session = vi.hoisted(() => ({
  provider: "claude" as AgentId,
  displayName: "Claude",
  readsHooks: true,
  filter: "",
}));

vi.mock("@/features/session/SessionProvider", () => ({
  useAgentSession: () => {
    const dto = { ...emptyTabDto(), hooks: [hook] };
    return {
      currentAgent: {
        id: session.provider,
        displayName: session.displayName,
        readsHooks: session.readsHooks,
      },
      currentTab: { dto, filter: session.filter, inFlight: false, loading: false, error: null },
      // The shell filters once, for every screen; the route only picks its slice out.
      filtered: filterTab(dto, session.filter),
      emptyTabDto,
    };
  },
}));

describe("HooksRoute", () => {
  beforeEach(() => {
    session.provider = "claude";
    session.displayName = "Claude";
    session.readsHooks = true;
    session.filter = "";
  });

  it("lists the provider's hooks", () => {
    render(<HooksRoute />);
    expect(screen.getByText("PreToolUse")).toBeInTheDocument();
  });

  it("keeps a row the shell's filter matches", () => {
    session.filter = "guard.sh";
    render(<HooksRoute />);

    expect(screen.getByText("PreToolUse")).toBeInTheDocument();
    expect(screen.queryByText(/Nothing matches/)).toBeNull();
  });

  it("drops a row the shell's filter misses", () => {
    session.filter = "postoolu";
    render(<HooksRoute />);

    expect(screen.queryByText("PreToolUse")).toBeNull();
    expect(screen.getByText(/Nothing matches/)).toBeInTheDocument();
  });

  it("says so for a provider whose hooks on-n-off does not read", () => {
    session.provider = "cursor";
    session.displayName = "Cursor";
    session.readsHooks = false;
    render(<HooksRoute />);

    expect(screen.getByText(/doesn’t read hooks for Cursor/)).toBeInTheDocument();
    expect(screen.queryByText("PreToolUse")).toBeNull();
  });
});
