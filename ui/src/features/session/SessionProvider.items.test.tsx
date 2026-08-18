import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionProvider, useAgentSession } from "./SessionProvider";
import type { AgentId, AgentInfo, AgentTabDto, InstallItemsRequest, InstallItemsResult } from "$lib/types";

const state = vi.hoisted(() => ({
  listCalls: [] as AgentId[],
  installResult: null as InstallItemsResult | null,
  installError: null as unknown,
}));

const EMPTY_TAB: AgentTabDto = { plugins: [], userSkills: [], mcpServers: [] };
const AGENTS: AgentInfo[] = (["claude", "codex", "antigravity", "cursor"] as const).map((id) => ({
  id,
  displayName: id,
  cliOk: true,
  cliError: null,
  installGit: true,
  installFolder: true,
  pluginToggle: true,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => Promise.resolve(null),
}));

vi.mock("$lib/api", () => ({
  featureFlags: () => Promise.resolve({ masterCut: false }),
  loadAppSettings: () => Promise.resolve({ hiddenAgents: [], binaryPaths: {} }),
  listAgents: () => Promise.resolve(AGENTS),
  listProjects: () => Promise.resolve([]),
  listLocalPlugins: (agentId: AgentId) => {
    state.listCalls.push(agentId);
    return Promise.resolve(EMPTY_TAB);
  },
  listPlugins: () => Promise.resolve(EMPTY_TAB),
  refresh: (agentId: AgentId) => {
    state.listCalls.push(agentId);
    return Promise.resolve(EMPTY_TAB);
  },
  installItems: (_request: InstallItemsRequest) =>
    state.installError ? Promise.reject(state.installError) : Promise.resolve(state.installResult),
}));

const REQUEST: InstallItemsRequest = {
  source: { owner: "mattpocock", repo: "skills", ref: "HEAD" },
  commitSha: "a".repeat(40),
  items: [{ pluginName: "p", kind: "skill", path: "skills/tdd", source: null }],
  targets: [
    { provider: "claude", scope: { kind: "global" } },
    { provider: "codex", scope: { kind: "global" } },
  ],
  overwriteUnmanaged: false,
};

function Probe() {
  const session = useAgentSession();
  return (
    <>
      <span data-testid="install-open">{String(session.installOpen)}</span>
      <span data-testid="install-error">{session.installError ?? ""}</span>
      <button type="button" onClick={() => session.setInstallOpen(true)}>
        Open
      </button>
      <button type="button" onClick={() => void session.installItems(REQUEST)}>
        Install items
      </button>
    </>
  );
}

function outcome(provider: AgentId, status: InstallItemsResult["outcomes"][number]["status"]) {
  return { provider, kind: "skill" as const, name: "tdd", targetPath: "x", status, reason: null };
}

describe("SessionProvider.installItems", () => {
  beforeEach(() => {
    const values = new Map<string, string>();
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        clear: () => values.clear(),
        getItem: (key: string) => values.get(key) ?? null,
        key: (index: number) => [...values.keys()][index] ?? null,
        get length() {
          return values.size;
        },
        removeItem: (key: string) => values.delete(key),
        setItem: (key: string, value: string) => values.set(key, String(value)),
      },
    });
    state.listCalls.length = 0;
    state.installResult = null;
    state.installError = null;
  });

  it("refreshes every touched provider and closes the sheet on a clean result", async () => {
    state.installResult = {
      commitSha: "a".repeat(40),
      shaMoved: false,
      outcomes: [outcome("claude", "installed"), outcome("codex", "replaced")],
    };
    render(
      <SessionProvider>
        <Probe />
      </SessionProvider>,
    );
    await waitFor(() => expect(state.listCalls.length).toBeGreaterThanOrEqual(4));
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByTestId("install-open").textContent).toBe("true");
    const before = state.listCalls.length;
    fireEvent.click(screen.getByRole("button", { name: "Install items" }));
    await waitFor(() => expect(screen.getByTestId("install-open").textContent).toBe("false"));
    await waitFor(() => {
      const after = state.listCalls.slice(before);
      expect(after).toContain("claude");
      expect(after).toContain("codex");
      expect(after).not.toContain("antigravity");
    });
  });

  it("keeps the sheet open on conflicts and surfaces invoke errors", async () => {
    state.installResult = {
      commitSha: "a".repeat(40),
      shaMoved: false,
      outcomes: [outcome("claude", "installed"), outcome("codex", "conflict")],
    };
    render(
      <SessionProvider>
        <Probe />
      </SessionProvider>,
    );
    await waitFor(() => expect(state.listCalls.length).toBeGreaterThanOrEqual(4));
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    fireEvent.click(screen.getByRole("button", { name: "Install items" }));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.getByTestId("install-open").textContent).toBe("true");

    state.installError = { kind: "message", message: "boom", path: null };
    fireEvent.click(screen.getByRole("button", { name: "Install items" }));
    await waitFor(() => expect(screen.getByTestId("install-error").textContent).toContain("boom"));
    expect(screen.getByTestId("install-open").textContent).toBe("true");
  });
});
