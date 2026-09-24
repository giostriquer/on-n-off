import { render, screen, within } from "@testing-library/react";
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

/** The row that names `name`. */
function row(name: string): HTMLElement {
  return screen.getByText(name).closest("article")!;
}

/** The dot beside a row's name: lit when the server runs here. */
function liveDot(article: HTMLElement): Element {
  return article.querySelector("span.rounded-full")!;
}

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

  it("labels a plugin's server with its plugin and a per-project server with its projects", () => {
    const tracker: McpServerDto = {
      ...server,
      id: "plugin:kit:tracker",
      name: "tracker",
      togglable: false,
      origin: "plugin",
      pluginId: "kit@acme",
    };
    const docs: McpServerDto = {
      ...server,
      id: "local:library-docs",
      name: "library-docs",
      togglable: false,
      origin: "local",
      projects: ["/Users/me/acme/webapp", "/Users/me/acme/api"],
    };
    const pad: McpServerDto = { ...docs, id: "local:pad", name: "pad", projects: ["/Users/me/acme/notes"] };
    // Its plugin is not listed (not installed from this home's inventory): the raw id stands in.
    const orphan: McpServerDto = { ...tracker, id: "plugin:gone:orphan", name: "orphan", pluginId: "gone@acme" };
    const plugin = (id: string, name: string) => ({
      id,
      name,
      source: "acme",
      version: "",
      upstream: "",
      enabled: true,
      togglable: true,
      skills: [],
    });
    // Another plugin listed first, so only a lookup by id finds Kit.
    const plugins = [plugin("tools@acme", "Tools"), plugin("kit@acme", "Kit")];
    const servers = [server, tracker, docs, pad, orphan];
    render(<McpList tab={{ plugins, userSkills: [], mcpServers: servers }} servers={servers} onToggle={vi.fn()} />);

    expect(screen.getByText("3 live · user config + plugins + per-project · handshake not probed")).toBeInTheDocument();
    const trackerRow = row("tracker");
    expect(within(trackerRow).getByText("PLUGIN")).toBeInTheDocument();
    expect(within(trackerRow).getByText("from Kit")).toBeInTheDocument();
    expect(within(row("orphan")).getByText("from gone@acme")).toBeInTheDocument();
    expect(within(row("library-docs")).getByText("PER-PROJECT")).toBeInTheDocument();
    const projects = within(row("library-docs")).getByText("in 2 projects");
    expect(projects).toHaveAttribute("title", "/Users/me/acme/api\n/Users/me/acme/webapp");
    expect(within(row("pad")).getByText("in notes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /tracker on/ })).toBeDisabled();
    // A server kept for particular projects does not run here: listed on, but no live glow.
    expect(liveDot(row("library-docs"))).not.toHaveClass("bg-[var(--live)]");
    expect(liveDot(row("tracker"))).toHaveClass("bg-[var(--live)]");
  });

  it("labels the user's own server with nothing and a project's server with exactly PROJECT", () => {
    const repo: McpServerDto = { ...server, id: "project:repo-docs", name: "repo-docs", togglable: false, origin: "project" };
    const servers = [server, repo];
    render(<McpList tab={{ plugins: [], userSkills: [], mcpServers: servers }} servers={servers} onToggle={vi.fn()} />);

    // The name line holds the name and its badges, nothing else; the block under it is that line
    // and the source, with no "from"/"in" line between them.
    const nameLine = (name: string) => screen.getByText(name).parentElement!;
    expect(nameLine("GitHub")).toHaveTextContent(/^GitHubSTDIO$/);
    expect(nameLine("GitHub").parentElement!.children).toHaveLength(2);
    expect(nameLine("repo-docs")).toHaveTextContent(/^repo-docsSTDIOPROJECT$/);
    expect(nameLine("repo-docs").parentElement!.children).toHaveLength(2);
  });

  it("keeps the plain header when every server is the user's own", () => {
    render(<McpList tab={{ plugins: [], userSkills: [], mcpServers: [server] }} servers={[server]} onToggle={vi.fn()} />);

    expect(screen.getByText("1 live · user-scope config only · handshake not probed")).toBeInTheDocument();
  });
});
