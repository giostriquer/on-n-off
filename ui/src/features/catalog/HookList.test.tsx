import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { HookList } from "./HookList";
import { emptyTabDto } from "$lib/catalog";
import type { AgentTabDto, HookDto } from "$lib/types";

function hook(overrides: Partial<HookDto> = {}): HookDto {
  return {
    id: "claude:settings.json:PreToolUse:0:0",
    event: "PreToolUse",
    matcher: "Bash",
    handler: "command",
    command: "${CLAUDE_PLUGIN_ROOT}/bin/guard.sh --strict --log /tmp/acme/guard.log",
    source: "settings.json",
    pluginId: null,
    description: "",
    enabled: true,
    ...overrides,
  };
}

function tabWith(hooks: HookDto[]): AgentTabDto {
  return { ...emptyTabDto(), hooks };
}

describe("HookList", () => {
  it("renders one row per handler with its event, matcher, source and handler type", () => {
    const hooks = [
      hook(),
      hook({
        id: "claude:acme-guardrails:PostToolUse:0:0",
        event: "PostToolUse",
        matcher: "",
        handler: "mcp_tool",
        command: "acme-review · lint_diff",
        source: "acme-guardrails",
        pluginId: "acme-guardrails@webapp",
        description: "Lints every diff the agent writes.",
      }),
    ];
    render(<HookList tab={tabWith(hooks)} hooks={hooks} />);

    const rows = screen.getAllByRole("article");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText("PreToolUse")).toBeInTheDocument();
    expect(within(rows[0]).getByText("Bash")).toBeInTheDocument();
    expect(within(rows[0]).getByText("settings.json")).toBeInTheDocument();
    expect(within(rows[0]).getByText("COMMAND")).toBeInTheDocument();
    expect(within(rows[1]).getByText("PostToolUse")).toBeInTheDocument();
    expect(within(rows[1]).getByText("acme-guardrails")).toBeInTheDocument();
    expect(within(rows[1]).getByText("Lints every diff the agent writes.")).toBeInTheDocument();
    expect(within(rows[1]).getByText("MCP TOOL")).toBeInTheDocument();
  });

  it("keeps the command on one line and gives the full text on click", async () => {
    const user = userEvent.setup();
    const entry = hook();
    render(<HookList tab={tabWith([entry])} hooks={[entry]} />);

    const trigger = screen.getByRole("button", { name: /full command/i });
    expect(trigger).toHaveClass("truncate");
    await user.click(trigger);
    expect(screen.getByRole("tooltip")).toHaveTextContent(entry.command);
  });

  it("reads a disabled Codex hook as inactive", () => {
    const off = hook({
      id: "codex:config.toml:notification:0:0",
      event: "Notification",
      matcher: "",
      source: "config.toml",
      enabled: false,
    });
    const on = hook();
    render(<HookList tab={tabWith([on, off])} hooks={[on, off]} />);

    const rows = screen.getAllByRole("article");
    expect(within(rows[1]).getByText("OFF")).toBeInTheDocument();
    expect(within(rows[0]).queryByText("OFF")).toBeNull();
  });

  it("renders exactly the rows it is handed, filter or not", () => {
    const entry = hook();
    render(<HookList tab={tabWith([entry, hook({ id: "other", event: "Stop" })])} hooks={[entry]} filterQuery="pre" />);

    expect(screen.getAllByRole("article")).toHaveLength(1);
    expect(screen.queryByText("Stop")).toBeNull();
  });

  it("counts the whole pool, not the filtered rows, and says what the pool covers", () => {
    const shown = hook();
    const pool = [shown, hook({ id: "claude:settings.json:Stop:0:0", event: "Stop" }), hook({ id: "off", enabled: false })];
    render(<HookList tab={tabWith(pool)} hooks={[shown]} filterQuery="pretool" />);

    // Two of the three are live, and the filter narrows the rows without moving the tally.
    expect(screen.getByText(/2 active/)).toHaveTextContent("2 active · user settings + plugins · listed, never run");
    expect(screen.getAllByRole("article")).toHaveLength(1);
  });

  it("offers nothing to switch: the screen reads hooks and never writes them", () => {
    const hooks = [hook(), hook({ id: "off", enabled: false })];
    render(<HookList tab={tabWith(hooks)} hooks={hooks} />);

    expect(screen.queryAllByRole("switch")).toHaveLength(0);
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("says a provider's hooks are not read instead of showing it as unconfigured", () => {
    render(<HookList tab={emptyTabDto()} hooks={[]} unread="on-n-off doesn’t read hooks for Cursor yet." />);

    expect(screen.getByText("on-n-off doesn’t read hooks for Cursor yet.")).toBeInTheDocument();
    expect(screen.queryByText("No hooks on this circuit.")).toBeNull();
  });

  it("separates no hooks configured from nothing matching the filter", () => {
    const { rerender } = render(<HookList tab={emptyTabDto()} hooks={[]} />);
    expect(screen.getByText("No hooks on this circuit.")).toBeInTheDocument();

    rerender(<HookList tab={tabWith([hook()])} hooks={[]} filterQuery="zzz" />);
    expect(screen.getByText(/Nothing matches/)).toBeInTheDocument();
  });
});
