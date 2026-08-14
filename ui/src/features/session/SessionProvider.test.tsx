import { StrictMode } from "react";
import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AGENT_KEY, SessionProvider } from "./SessionProvider";
import type { AgentId, AgentInfo, AgentTabDto } from "$lib/types";

const state = vi.hoisted(() => ({
  refreshOrder: [] as AgentId[],
  resolveSelected: null as null | ((value: AgentTabDto) => void),
}));

const EMPTY_TAB: AgentTabDto = { plugins: [], userSkills: [], mcpServers: [] };
const AGENTS: AgentInfo[] = [
  {
    id: "claude",
    displayName: "Claude",
    cliOk: true,
    cliError: null,
    installGit: true,
    installFolder: true,
    pluginToggle: true,
  },
  {
    id: "codex",
    displayName: "Codex",
    cliOk: true,
    cliError: null,
    installGit: true,
    installFolder: true,
    pluginToggle: true,
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    cliOk: true,
    cliError: null,
    installGit: true,
    installFolder: true,
    pluginToggle: true,
  },
];

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => Promise.resolve(null),
}));

vi.mock("$lib/api", () => ({
  featureFlags: () => Promise.resolve({ masterCut: false }),
  loadAppSettings: () => Promise.resolve({ hiddenAgents: [], binaryPaths: {} }),
  listAgents: () => Promise.resolve(AGENTS),
  listProjects: () => Promise.resolve([]),
  refresh: (agentId: AgentId) => {
    state.refreshOrder.push(agentId);
    if (agentId === "codex") {
      return new Promise<AgentTabDto>((resolve) => {
        state.resolveSelected = resolve;
      });
    }
    return Promise.resolve(EMPTY_TAB);
  },
}));

describe("SessionProvider startup", () => {
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
    localStorage.setItem(AGENT_KEY, "codex");
    state.refreshOrder.length = 0;
    state.resolveSelected = null;
  });

  it("finishes the selected provider before starting background providers", async () => {
    render(
      <StrictMode>
        <SessionProvider>
          <span>session</span>
        </SessionProvider>
      </StrictMode>,
    );

    await waitFor(() => expect(state.resolveSelected).not.toBeNull());
    expect(state.refreshOrder).toEqual(["codex"]);

    state.resolveSelected?.(EMPTY_TAB);
    await waitFor(() =>
      expect(state.refreshOrder).toEqual(["codex", "claude", "antigravity"]),
    );
  });
});
