import { StrictMode, useRef } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AGENT_KEY, SessionProvider, useAgentSession } from "./SessionProvider";
import type { AgentId, AgentInfo, AgentTabDto, ProjectDto } from "$lib/types";

const state = vi.hoisted(() => ({
  refreshOrder: [] as AgentId[],
  resolveSelected: null as null | ((value: AgentTabDto) => void),
  refreshResults: {} as Record<AgentId, AgentTabDto>,
  projectResults: {} as Record<AgentId, ProjectDto[]>,
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

const CODEX_TAB: AgentTabDto = {
  plugins: [
    {
      id: "workbench@workshop",
      name: "workbench",
      source: "workshop",
      version: "0.23.0",
      upstream: "0.23.0",
      enabled: true,
      togglable: true,
      skills: [
        {
          id: "workbench@workshop:brainstorming",
          pluginId: "workbench@workshop",
          name: "brainstorming",
          description: "Turn ideas into designs",
          enabled: true,
          togglable: false,
        },
      ],
    },
  ],
  userSkills: [],
  mcpServers: [],
};

const CLAUDE_TAB: AgentTabDto = {
  plugins: [
    {
      id: "frontend@local",
      name: "frontend",
      source: "local",
      version: "1.0.0",
      upstream: "1.0.0",
      enabled: true,
      togglable: true,
      skills: [],
    },
  ],
  userSkills: [],
  mcpServers: [],
};

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => Promise.resolve(null),
}));

vi.mock("$lib/api", () => ({
  featureFlags: () => Promise.resolve({ masterCut: false }),
  loadAppSettings: () => Promise.resolve({ hiddenAgents: [], binaryPaths: {} }),
  listAgents: () => Promise.resolve(AGENTS),
  listProjects: (agentId: AgentId) => Promise.resolve(state.projectResults[agentId] ?? []),
  refresh: (agentId: AgentId) => {
    state.refreshOrder.push(agentId);
    if (agentId === "codex") {
      return new Promise<AgentTabDto>((resolve) => {
        state.resolveSelected = resolve;
      });
    }
    return Promise.resolve(state.refreshResults[agentId] ?? EMPTY_TAB);
  },
}));

function SessionProbe() {
  const session = useAgentSession();
  const derivation = useRef({ value: session.filtered, count: 0 });
  if (derivation.current.value !== session.filtered) {
    derivation.current = { value: session.filtered, count: derivation.current.count + 1 };
  }
  const projectDerivation = useRef({ value: session.currentProjects, count: 0 });
  if (projectDerivation.current.value !== session.currentProjects) {
    projectDerivation.current = {
      value: session.currentProjects,
      count: projectDerivation.current.count + 1,
    };
  }
  return (
    <>
      <span data-testid="selected-provider">{session.selected}</span>
      <span data-testid="catalog-derivations">{derivation.current.count}</span>
      <span data-testid="filtered-plugins">
        {session.filtered?.plugins.map((plugin) => plugin.id).join(",") ?? ""}
      </span>
      <span data-testid="expanded-plugins">{[...session.expandedIds].join(",")}</span>
      <span data-testid="project-derivations">{projectDerivation.current.count}</span>
      <span data-testid="scope-label">{session.currentScopeLabel}</span>
      <input
        aria-label="Session filter"
        value={session.currentTab.filter}
        onChange={(event) => session.setFilter(event.target.value)}
      />
      <button type="button" onClick={session.toggleTheme}>
        Toggle theme
      </button>
      <button type="button" onClick={() => session.setSelected("codex")}>
        Select Codex
      </button>
      <button type="button" onClick={() => session.setSelected("claude")}>
        Select Claude
      </button>
      <button type="button" onClick={() => void session.loadTab("claude")}>
        Reload Claude
      </button>
    </>
  );
}

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
    state.refreshResults = {
      claude: CLAUDE_TAB,
      codex: CODEX_TAB,
      antigravity: EMPTY_TAB,
    };
    state.projectResults = { claude: [], codex: [], antigravity: [] };
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

  it("reuses catalog derivation for unrelated state and derives once for a filter change", async () => {
    render(
      <SessionProvider>
        <SessionProbe />
      </SessionProvider>,
    );
    await waitFor(() => expect(state.resolveSelected).not.toBeNull());
    state.resolveSelected?.(CODEX_TAB);
    await waitFor(() => expect(state.refreshOrder).toEqual(["codex", "claude", "antigravity"]));
    await waitFor(() => expect(screen.getByTestId("filtered-plugins")).toHaveTextContent("workbench@workshop"));
    const baseline = Number(screen.getByTestId("catalog-derivations").textContent);

    fireEvent.click(screen.getByRole("button", { name: "Toggle theme" }));
    expect(screen.getByTestId("catalog-derivations")).toHaveTextContent(String(baseline));

    fireEvent.change(screen.getByRole("textbox", { name: "Session filter" }), {
      target: { value: "brainstorm" },
    });
    await waitFor(() =>
      expect(screen.getByTestId("expanded-plugins")).toHaveTextContent("workbench@workshop"),
    );
    expect(screen.getByTestId("catalog-derivations")).toHaveTextContent(String(baseline + 1));

    fireEvent.change(screen.getByRole("textbox", { name: "Session filter" }), {
      target: { value: "" },
    });
    await waitFor(() => expect(screen.getByTestId("filtered-plugins")).toHaveTextContent("workbench@workshop"));
  });

  it("keeps each provider filter independent", async () => {
    render(
      <SessionProvider>
        <SessionProbe />
      </SessionProvider>,
    );
    await waitFor(() => expect(state.resolveSelected).not.toBeNull());
    state.resolveSelected?.(CODEX_TAB);
    await waitFor(() => expect(state.refreshOrder).toEqual(["codex", "claude", "antigravity"]));

    fireEvent.change(screen.getByRole("textbox", { name: "Session filter" }), {
      target: { value: "workbench" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Select Claude" }));
    await waitFor(() => expect(screen.getByTestId("selected-provider")).toHaveTextContent("claude"));
    expect(screen.getByRole("textbox", { name: "Session filter" })).toHaveValue("");

    fireEvent.change(screen.getByRole("textbox", { name: "Session filter" }), {
      target: { value: "frontend" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Select Codex" }));
    await waitFor(() => expect(screen.getByTestId("selected-provider")).toHaveTextContent("codex"));
    expect(screen.getByRole("textbox", { name: "Session filter" })).toHaveValue("workbench");
  });

  it("keeps the selected project and scope view stable when another provider reloads", async () => {
    render(
      <SessionProvider>
        <SessionProbe />
      </SessionProvider>,
    );
    await waitFor(() => expect(state.resolveSelected).not.toBeNull());
    state.resolveSelected?.(CODEX_TAB);
    await waitFor(() => expect(state.refreshOrder).toEqual(["codex", "claude", "antigravity"]));
    const baseline = screen.getByTestId("project-derivations").textContent;
    expect(screen.getByTestId("scope-label")).toHaveTextContent("all projects");

    state.projectResults.claude = [
      { id: "claude-project", label: "Claude project", path: "C:/fixture/claude-project" },
    ];
    fireEvent.click(screen.getByRole("button", { name: "Reload Claude" }));
    await waitFor(() =>
      expect(state.refreshOrder).toEqual(["codex", "claude", "antigravity", "claude"]),
    );

    expect(screen.getByTestId("project-derivations")).toHaveTextContent(baseline ?? "0");
    expect(screen.getByTestId("scope-label")).toHaveTextContent("all projects");
  });
});
