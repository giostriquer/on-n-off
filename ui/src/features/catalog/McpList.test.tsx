import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { McpList } from "./McpList";
import type { McpServerDto } from "$lib/types";

const filterMcpListCall = vi.hoisted(() => vi.fn());

vi.mock("$lib/filterTab", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/filterTab")>();
  return {
    ...actual,
    filterMcpList: (...args: Parameters<typeof actual.filterMcpList>) => {
      filterMcpListCall();
      return actual.filterMcpList(...args);
    },
  };
});

const server: McpServerDto = {
  id: "github",
  name: "GitHub",
  system: "stdio",
  source: "npx github-mcp",
  enabled: true,
  togglable: true,
};

describe("McpList", () => {
  it("uses an already-derived server list without filtering it again", () => {
    render(
      <McpList
        tab={{ plugins: [], userSkills: [], mcpServers: [server] }}
        servers={[server]}
        filterQuery="github"
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getByText("GitHub")).toBeInTheDocument();
    expect(filterMcpListCall).not.toHaveBeenCalled();
  });

  it("shows a provider notice and keeps read-only servers unswitchable", () => {
    const readOnly = { ...server, togglable: false };
    const onToggle = vi.fn();
    render(
      <McpList
        tab={{ plugins: [], userSkills: [], mcpServers: [readOnly] }}
        servers={[readOnly]}
        notice="Managed in Cursor."
        onToggle={onToggle}
      />,
    );

    expect(screen.getByRole("note")).toHaveTextContent("Managed in Cursor.");
    expect(screen.getByRole("button", { name: /GitHub on/ })).toBeDisabled();
  });

  it("labels plugin and per-project servers and counts only what is live here", () => {
    const tracker: McpServerDto = {
      ...server,
      id: "plugin:kit:tracker",
      name: "tracker",
      togglable: false,
      origin: "plugin",
      via: "kit",
    };
    const docs: McpServerDto = {
      ...server,
      id: "local:library-docs",
      name: "library-docs",
      togglable: false,
      origin: "local",
      via: "2 projects",
    };
    const servers = [server, tracker, docs];
    render(
      <McpList tab={{ plugins: [], userSkills: [], mcpServers: servers }} servers={servers} onToggle={vi.fn()} />,
    );

    expect(screen.getByText("2 live · user config + plugins + per-project · handshake not probed")).toBeInTheDocument();
    const trackerRow = screen.getByText("tracker").closest("article")!;
    expect(trackerRow).toHaveTextContent("PLUGIN");
    expect(trackerRow).toHaveTextContent("from kit");
    const docsRow = screen.getByText("library-docs").closest("article")!;
    expect(docsRow).toHaveTextContent("PER-PROJECT");
    expect(docsRow).toHaveTextContent("in 2 projects");
    expect(screen.getByRole("button", { name: /tracker on/ })).toBeDisabled();
  });

  it("keeps the plain header when every server is the user's own", () => {
    render(<McpList tab={{ plugins: [], userSkills: [], mcpServers: [server] }} servers={[server]} onToggle={vi.fn()} />);

    expect(screen.getByText("1 live · user-scope config only · handshake not probed")).toBeInTheDocument();
  });
});
