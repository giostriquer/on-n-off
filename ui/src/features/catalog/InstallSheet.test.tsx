import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { InstallSheet } from "./InstallSheet";
import type { AgentInfo, InstallItemsRequest, InstallItemsResult, MarketplaceInspect } from "$lib/types";

const api = vi.hoisted(() => ({
  inspectMarketplace: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  inspectMarketplace: api.inspectMarketplace,
}));

const AGENTS: AgentInfo[] = (["claude", "codex", "antigravity", "cursor"] as const).map((id) => ({
  id,
  displayName: id[0].toUpperCase() + id.slice(1),
  cliOk: true,
  cliError: null,
  installGit: id !== "cursor",
  installFolder: id !== "cursor",
  pluginToggle: id !== "cursor",
}));

const INSPECT: MarketplaceInspect = {
  isMarketplace: true,
  commitSha: "a".repeat(40),
  marketplaceName: "mattpocock",
  hint: null,
  plugins: [
    {
      name: "mattpocock-skills",
      version: "1.2.3",
      description: "Matt's skills",
      supported: true,
      source: null,
      skills: [
        { name: "tdd", description: "Test-driven development", path: "skills/engineering/tdd" },
        { name: "grilling", description: "Ask hard questions", path: "skills/productivity/grilling" },
      ],
      agents: [{ name: "reviewer", description: "Reviews code", path: "agents/reviewer.md" }],
    },
  ],
};

function Providers({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function renderSheet(overrides: Partial<Parameters<typeof InstallSheet>[0]> = {}) {
  const onInstall = vi.fn();
  const onInstallItems = vi.fn<(request: InstallItemsRequest) => Promise<InstallItemsResult | null>>(
    async () => null,
  );
  const utils = render(
    <Providers>
      <InstallSheet
        agentName="Claude"
        busy={false}
        error={null}
        installFolder
        visibleAgents={AGENTS}
        currentAgentId="claude"
        projects={[{ id: "p", label: "app", path: "E:/dev/app" }]}
        currentScopePath={null}
        onCancel={() => {}}
        onInstall={onInstall}
        onInstallItems={onInstallItems}
        onPickFolder={async () => null}
        {...overrides}
      />
    </Providers>,
  );
  return { ...utils, onInstall, onInstallItems };
}

function typeSource(value: string) {
  fireEvent.change(screen.getByLabelText("Install source"), { target: { value } });
}

describe("InstallSheet marketplace browsing", () => {
  beforeEach(() => {
    api.inspectMarketplace.mockReset();
    api.inspectMarketplace.mockResolvedValue(INSPECT);
  });

  it("keeps the plain flow for plugin ids and shows the three actions for a GitHub repo", async () => {
    const { onInstall } = renderSheet();
    typeSource("workbench@workshop");
    expect(screen.queryByRole("radio")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    expect(onInstall).toHaveBeenCalledWith("workbench@workshop");

    typeSource("mattpocock/skills");
    const radios = await screen.findAllByRole("radio");
    expect(radios).toHaveLength(3);
    expect(api.inspectMarketplace).toHaveBeenCalledWith("mattpocock", "skills", undefined);
    // Default stays "Install plugin" so the CLI path is one click away as before.
    expect(screen.getByRole("radio", { name: /Install plugin/ })).toBeChecked();
  });

  it("selects items per group, gates agents on Claude, and submits picks and targets", async () => {
    const { onInstallItems } = renderSheet();
    typeSource("mattpocock/skills");
    fireEvent.click(await screen.findByRole("radio", { name: /Install selected/ }));
    const tree = screen.getByRole("group", { name: "mattpocock-skills" });
    expect(within(tree).getByRole("checkbox", { name: /tdd/ })).not.toBeChecked();
    fireEvent.click(within(tree).getAllByRole("button", { name: "all" })[0]);
    expect(within(tree).getByRole("checkbox", { name: /tdd/ })).toBeChecked();
    expect(within(tree).getByRole("checkbox", { name: /grilling/ })).toBeChecked();
    fireEvent.click(within(tree).getByRole("checkbox", { name: /grilling/ }));
    fireEvent.click(within(tree).getByRole("checkbox", { name: /reviewer/ }));

    // Drop Claude → agents are no longer allowed.
    fireEvent.click(screen.getByRole("checkbox", { name: "Claude" }));
    expect(within(tree).getByRole("checkbox", { name: /reviewer/ })).toBeDisabled();
    expect(screen.getByText(/Subagents only install into Claude/)).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "Codex" }));

    fireEvent.click(screen.getByRole("button", { name: /Install 1 item/ }));
    await waitFor(() => expect(onInstallItems).toHaveBeenCalledTimes(1));
    const request = onInstallItems.mock.calls[0][0];
    expect(request.source).toEqual({ owner: "mattpocock", repo: "skills", ref: "HEAD" });
    expect(request.commitSha).toBe("a".repeat(40));
    expect(request.items).toEqual([
      { pluginName: "mattpocock-skills", kind: "skill", path: "skills/engineering/tdd", source: null },
    ]);
    expect(request.targets).toEqual([{ provider: "codex", scope: { kind: "global" } }]);
    expect(request.overwriteUnmanaged).toBe(false);
  });

  it("offers to overwrite conflicts and re-submits with the flag", async () => {
    const conflict: InstallItemsResult = {
      commitSha: "a".repeat(40),
      shaMoved: false,
      outcomes: [
        {
          provider: "claude",
          kind: "skill",
          name: "tdd",
          pluginName: "mattpocock-skills",
          path: "skills/engineering/tdd",
          targetPath: "x",
          status: "installed",
          reason: null,
        },
        {
          provider: "claude",
          kind: "skill",
          name: "grilling",
          pluginName: "mattpocock-skills",
          path: "skills/productivity/grilling",
          targetPath: "y",
          status: "conflict",
          reason: "exists",
        },
      ],
    };
    const { onInstallItems } = renderSheet();
    onInstallItems.mockResolvedValueOnce(conflict);
    typeSource("mattpocock/skills");
    fireEvent.click(await screen.findByRole("radio", { name: /Install everything/ }));
    fireEvent.click(screen.getByRole("button", { name: /Install 3 items/ }));
    const overwrite = await screen.findByRole("button", { name: "Overwrite this one?" });
    expect(within(screen.getByRole("status")).getByText(/grilling · claude/)).toBeTruthy();
    fireEvent.click(overwrite);
    await waitFor(() => expect(onInstallItems).toHaveBeenCalledTimes(2));
    const retry = onInstallItems.mock.calls[1][0];
    expect(retry.overwriteUnmanaged).toBe(true);
    expect(retry.items.map((i) => i.path)).toEqual(["skills/productivity/grilling"]);
  });

  it("carries the requested ref and the chosen scope into the request", async () => {
    const { onInstallItems } = renderSheet({ currentScopePath: "E:/dev/app" });
    typeSource("mattpocock/skills@v1");
    fireEvent.click(await screen.findByRole("radio", { name: /Install selected/ }));
    expect(api.inspectMarketplace).toHaveBeenCalledWith("mattpocock", "skills", "v1");
    // Scope defaults to the Scope bar's project and can be switched back to Global.
    expect(screen.getByRole("button", { name: "Project" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("E:/dev/app/.claude/skills")).toBeTruthy();
    const tree = screen.getByRole("group", { name: "mattpocock-skills" });
    fireEvent.click(within(tree).getByRole("checkbox", { name: /tdd/ }));
    fireEvent.click(screen.getByRole("button", { name: /Install 1 item/ }));
    await waitFor(() => expect(onInstallItems).toHaveBeenCalledTimes(1));
    const first = onInstallItems.mock.calls[0][0];
    expect(first.source).toEqual({ owner: "mattpocock", repo: "skills", ref: "v1" });
    expect(first.targets).toEqual([{ provider: "claude", scope: { kind: "project", projectPath: "E:/dev/app" } }]);

    fireEvent.click(screen.getByRole("button", { name: "Global" }));
    expect(screen.getByText("~/.claude/skills")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Install 1 item/ }));
    await waitFor(() => expect(onInstallItems).toHaveBeenCalledTimes(2));
    expect(onInstallItems.mock.calls[1][0].targets).toEqual([{ provider: "claude", scope: { kind: "global" } }]);
  });

  it("tells a non-marketplace repo apart from a download failure", async () => {
    api.inspectMarketplace.mockResolvedValueOnce({
      ...INSPECT,
      isMarketplace: false,
      plugins: [],
      hint: "No plugin marketplace manifest",
    });
    renderSheet();
    typeSource("acme/tools");
    expect(await screen.findByText(/No plugin marketplace in this repository/)).toBeTruthy();
    expect(screen.queryByRole("radio")).toBeNull();

    api.inspectMarketplace.mockRejectedValueOnce({ kind: "message", message: "download failed: boom", path: null });
    typeSource("acme/other");
    expect(await screen.findByText(/Couldn’t download the marketplace/)).toBeTruthy();
  });
});
