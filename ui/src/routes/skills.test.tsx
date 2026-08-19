import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SkillsRoute } from "./skills";
import type { AgentId, ItemStatus } from "$lib/types";

const api = vi.hoisted(() => ({
  itemUpdateStatus: vi.fn(),
  updateItem: vi.fn(),
  removeItem: vi.fn(),
}));
const session = vi.hoisted(() => ({
  provider: "claude" as AgentId,
  loadTab: vi.fn(async () => {}),
}));

vi.mock("$lib/api", () => api);
vi.mock("@/features/session/SessionProvider", () => ({
  useAgentSession: () => ({
    currentAgent: {
      id: session.provider,
      displayName: "Claude",
      cliOk: true,
      cliError: null,
      installGit: true,
      installFolder: true,
      pluginToggle: true,
    },
    currentScopePath: null,
    currentTab: { dto: null, inFlight: false, filter: "", loading: false, error: null },
    filtered: {
      skills: [
        { id: "user:tdd", pluginId: null, name: "tdd", description: "", enabled: true, togglable: true, origin: "user" },
      ],
    },
    expandedIds: new Set<string>(),
    emptyTabDto: () => ({ plugins: [], userSkills: [], mcpServers: [] }),
    toggleExpand: vi.fn(),
    togglePlugin: vi.fn(),
    toggleSkill: vi.fn(),
    setUninstallTarget: vi.fn(),
    loadTab: session.loadTab,
  }),
}));

function status(overrides: Partial<ItemStatus>): ItemStatus {
  return {
    id: "claude:skill:/x/tdd",
    provider: "claude",
    kind: "skill",
    name: "tdd",
    displayName: "tdd",
    targetPath: "/x/tdd",
    installedVersion: "1.2.3",
    installedSha: "a".repeat(40),
    modified: false,
    missing: false,
    upstream: { state: "updateAvailable", commitSha: "b".repeat(40), pluginVersion: "1.3.0" },
    source: { owner: "acme", repo: "skills", ref: "HEAD" },
    pluginName: "acme-skills",
    upstreamPath: "skills/ops/tdd",
    upstreamUrl: `https://github.com/acme/skills/tree/${"a".repeat(40)}/skills/ops/tdd`,
    ...overrides,
  };
}

function renderRoute() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <SkillsRoute />
    </QueryClientProvider>,
  );
}

describe("SkillsRoute managed items", () => {
  beforeEach(() => {
    api.itemUpdateStatus.mockReset();
    api.updateItem.mockReset();
    api.removeItem.mockReset();
    session.loadTab.mockClear();
    session.provider = "claude";
    api.itemUpdateStatus.mockResolvedValue([
      status({}),
      status({ id: "claude:agent:/x/reviewer.md", kind: "agent", name: "reviewer", displayName: "reviewer", targetPath: "/x/reviewer.md", upstream: { state: "current" } }),
    ]);
    api.updateItem.mockResolvedValue(status({ upstream: { state: "current" } }));
    api.removeItem.mockResolvedValue(undefined);
  });

  it("lists Claude subagents, offers Keep mine / Overwrite, and reloads the tab after acting", async () => {
    renderRoute();
    expect(await screen.findByText("update available → v1.3.0")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Subagents" })).toBeTruthy();
    expect(screen.getByText("reviewer")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Update tdd" }));
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    await waitFor(() => expect(api.updateItem).toHaveBeenCalledWith("claude:skill:/x/tdd", "dismiss"));
    await waitFor(() => expect(session.loadTab).toHaveBeenCalledWith("claude"));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Update tdd" }));
    fireEvent.click(screen.getByRole("button", { name: "Overwrite (backup kept)" }));
    await waitFor(() => expect(api.updateItem).toHaveBeenCalledWith("claude:skill:/x/tdd", "overwrite"));

    fireEvent.click(screen.getByRole("button", { name: "Remove reviewer" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(api.removeItem).toHaveBeenCalledWith("claude:agent:/x/reviewer.md"));
  });

  it("asks for a forced upstream check from the header button", async () => {
    renderRoute();
    await screen.findByText("update available → v1.3.0");
    expect(api.itemUpdateStatus).toHaveBeenLastCalledWith("claude", null, false);
    fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
    await waitFor(() => expect(api.itemUpdateStatus).toHaveBeenLastCalledWith("claude", null, true));
  });

  it("hides the Subagents section for other providers even when agent rows exist", async () => {
    session.provider = "codex";
    api.itemUpdateStatus.mockResolvedValue([
      status({ provider: "codex" }),
      status({ id: "codex:agent:x", provider: "codex", kind: "agent", name: "reviewer", displayName: "reviewer", upstream: { state: "current" } }),
    ]);
    renderRoute();
    await screen.findByText("update available → v1.3.0");
    expect(screen.queryByRole("region", { name: "Subagents" })).toBeNull();
  });
});
