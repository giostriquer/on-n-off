import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AGENT_KEY, SCOPE_KEY, SessionProvider, useAgentSession } from "./SessionProvider";
import type { AgentId, AgentInfo, AgentTabDto, ProjectDto } from "$lib/types";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const state = vi.hoisted(() => ({
  events: [] as string[],
  localHandler: null as null | ((agentId: AgentId, path: string | null) => Promise<AgentTabDto>),
  refreshHandler: null as null | ((agentId: AgentId, path: string | null) => Promise<AgentTabDto>),
  projectsHandler: null as null | ((agentId: AgentId) => Promise<ProjectDto[]>),
  inspectHandler: null as null | ((agentId: AgentId, path: string) => Promise<ProjectDto>),
  mutationHandler: null as null | ((agentId: AgentId) => Promise<AgentTabDto>),
}));

const EMPTY_TAB: AgentTabDto = { plugins: [], userSkills: [], mcpServers: [] };
const LOCAL_TAB: AgentTabDto = {
  plugins: [
    {
      id: "workbench@workshop",
      name: "workbench",
      source: "workshop",
      version: "0.22.0",
      upstream: "0.22.0",
      outOfSync: false,
      enabled: false,
      togglable: true,
      skills: [
        {
          id: "brainstorming",
          pluginId: "workbench@workshop",
          name: "brainstorming",
          description: "local skill",
          enabled: false,
          togglable: false,
        },
      ],
    },
  ],
  userSkills: [
    {
      id: "local-user-skill",
      pluginId: null,
      name: "local-user-skill",
      description: "local user skill",
      enabled: true,
      togglable: true,
    },
  ],
  mcpServers: [
    {
      id: "local-mcp",
      name: "local-mcp",
      system: "stdio",
      source: "local",
      enabled: false,
      togglable: true,
    },
  ],
};
const ENRICHED_TAB: AgentTabDto = {
  plugins: [
    {
      ...LOCAL_TAB.plugins[0],
      version: "0.23.0",
      upstream: "0.24.0",
      outOfSync: true,
      enabled: true,
      togglable: false,
      skills: [],
    },
  ],
  userSkills: [],
  mcpServers: [],
};
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
  loadAppSettings: () =>
    Promise.resolve({ hiddenAgents: [], binaryPaths: {}, automaticUpdates: true }),
  listAgents: () => Promise.resolve(AGENTS),
  listProjects: (agentId: AgentId) => {
    state.events.push(`projects:${agentId}`);
    return state.projectsHandler?.(agentId) ?? Promise.resolve([]);
  },
  inspectProject: (agentId: AgentId, path: string) => {
    state.events.push(`inspect:${agentId}:${path}`);
    return (
      state.inspectHandler?.(agentId, path) ??
      Promise.resolve({ id: path, label: path, path })
    );
  },
  listLocalPlugins: (agentId: AgentId, path: string | null) => {
    state.events.push(`local:${agentId}:${path ?? "global"}`);
    return state.localHandler?.(agentId, path) ?? Promise.resolve(EMPTY_TAB);
  },
  listPlugins: (agentId: AgentId, path: string | null) =>
    state.localHandler?.(agentId, path) ?? Promise.resolve(EMPTY_TAB),
  refresh: (agentId: AgentId, path: string | null) => {
    state.events.push(`refresh:${agentId}:${path ?? "global"}`);
    return state.refreshHandler?.(agentId, path) ?? Promise.resolve(EMPTY_TAB);
  },
  setPluginEnabled: (agentId: AgentId) => {
    state.events.push(`mutate:${agentId}`);
    return state.mutationHandler?.(agentId) ?? Promise.resolve(EMPTY_TAB);
  },
}));

function Probe() {
  const session = useAgentSession();
  const plugin = session.currentTab.dto?.plugins[0];
  return (
    <>
      <span data-testid="ready">{session.initialProviderReady ? "yes" : "no"}</span>
      <span data-testid="busy">{session.currentTab.inFlight ? "yes" : "no"}</span>
      <span data-testid="plugin-id">{plugin?.id ?? ""}</span>
      <span data-testid="plugin-version">{plugin?.version ?? ""}</span>
      <span data-testid="plugin-enabled">{String(plugin?.enabled ?? "")}</span>
      <span data-testid="plugin-skill">{plugin?.skills[0]?.description ?? ""}</span>
      <span data-testid="user-skills">
        {session.currentTab.dto?.userSkills.map((skill) => skill.id).join(",") ?? ""}
      </span>
      <span data-testid="mcp-servers">
        {session.currentTab.dto?.mcpServers.map((server) => server.id).join(",") ?? ""}
      </span>
      <span data-testid="scope-label">{session.currentScopeLabel}</span>
      <span data-testid="scope-path">{session.currentScopePath ?? "global"}</span>
      <span data-testid="projects">
        {session.currentProjects.map((project) => project.id).join(",")}
      </span>
      <span data-testid="banner">{session.banner ?? ""}</span>
      <button type="button" onClick={() => void session.loadTab("codex", true)}>
        Refresh Codex
      </button>
      <button type="button" onClick={() => void session.selectScope("C:/fixture/new-scope")}>
        Select new scope
      </button>
      <button
        type="button"
        onClick={() => plugin && session.togglePlugin(plugin, !plugin.enabled)}
      >
        Mutate Codex
      </button>
    </>
  );
}

function renderSession() {
  return render(
    <SessionProvider>
      <Probe />
    </SessionProvider>,
  );
}

describe("SessionProvider local-first startup", () => {
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
    Object.defineProperty(performance, "mark", { configurable: true, value: vi.fn() });
    localStorage.setItem(AGENT_KEY, "codex");
    state.events.length = 0;
    state.localHandler = (agentId) => Promise.resolve(agentId === "codex" ? LOCAL_TAB : EMPTY_TAB);
    state.refreshHandler = (agentId) =>
      Promise.resolve(agentId === "codex" ? ENRICHED_TAB : EMPTY_TAB);
    state.projectsHandler = () => Promise.resolve([]);
    state.inspectHandler = (_agentId, path) =>
      Promise.resolve({ id: path, label: "Remembered project", path });
    state.mutationHandler = (agentId) =>
      Promise.resolve(agentId === "codex" ? LOCAL_TAB : EMPTY_TAB);
  });

  it("renders selected local inventory before remote enrichment or bulk projects resolve", async () => {
    const projects = deferred<ProjectDto[]>();
    const enrichment = deferred<AgentTabDto>();
    state.projectsHandler = (agentId) =>
      agentId === "codex" ? projects.promise : Promise.resolve([]);
    state.refreshHandler = (agentId) =>
      agentId === "codex" ? enrichment.promise : Promise.resolve(EMPTY_TAB);

    renderSession();

    await waitFor(() => expect(screen.getByTestId("plugin-id")).toHaveTextContent("workbench@workshop"));
    expect(screen.getByTestId("ready")).toHaveTextContent("yes");
    expect(screen.getByTestId("busy")).toHaveTextContent("no");
    expect(screen.getByTestId("plugin-version")).toHaveTextContent("0.22.0");
    await waitFor(() => {
      expect(state.events).toContain("local:claude:global");
      expect(state.events).toContain("local:antigravity:global");
    });
    expect(state.events.indexOf("local:codex:global")).toBeLessThan(
      state.events.indexOf("local:claude:global"),
    );
    const marks = vi.mocked(performance.mark).mock.calls.map(([name]) => name);
    expect(marks.indexOf("on-n-off:selected-local-ready")).toBeLessThan(
      marks.indexOf("on-n-off:background-providers-start"),
    );

    projects.resolve([]);
    enrichment.resolve(ENRICHED_TAB);
  });

  it("applies only enriched plugin metadata and preserves the usable local catalog", async () => {
    const enrichment = deferred<AgentTabDto>();
    state.refreshHandler = (agentId) =>
      agentId === "codex" ? enrichment.promise : Promise.resolve(EMPTY_TAB);
    renderSession();
    await screen.findByText("workbench@workshop");

    enrichment.resolve(ENRICHED_TAB);

    await waitFor(() => expect(screen.getByTestId("plugin-version")).toHaveTextContent("0.23.0"));
    expect(screen.getByTestId("plugin-enabled")).toHaveTextContent("false");
    expect(screen.getByTestId("plugin-skill")).toHaveTextContent("local skill");
    expect(screen.getByTestId("user-skills")).toHaveTextContent("local-user-skill");
    expect(screen.getByTestId("mcp-servers")).toHaveTextContent("local-mcp");
  });

  it("keeps local inventory on enrichment failure and explicit Refresh fully replaces it", async () => {
    let codexRefreshes = 0;
    state.refreshHandler = (agentId) => {
      if (agentId !== "codex") {
        return Promise.resolve(EMPTY_TAB);
      }
      codexRefreshes += 1;
      return codexRefreshes === 1
        ? Promise.reject(new Error("remote fixture failed"))
        : Promise.resolve(ENRICHED_TAB);
    };
    renderSession();

    await waitFor(() => expect(screen.getByTestId("banner")).toHaveTextContent(/remote fixture failed/i));
    expect(screen.getByTestId("plugin-enabled")).toHaveTextContent("false");
    expect(screen.getByTestId("busy")).toHaveTextContent("no");

    fireEvent.click(screen.getByRole("button", { name: "Refresh Codex" }));

    await waitFor(() => expect(screen.getByTestId("plugin-enabled")).toHaveTextContent("true"));
    expect(screen.getByTestId("plugin-skill")).toBeEmptyDOMElement();
    expect(screen.getByTestId("user-skills")).toBeEmptyDOMElement();
    expect(screen.getByTestId("mcp-servers")).toBeEmptyDOMElement();
    expect(screen.getByTestId("banner")).toBeEmptyDOMElement();
  });

  it("waits for the remembered scope inspection but not bulk discovery", async () => {
    const rememberedPath = "C:/fixture/remembered";
    const projects = deferred<ProjectDto[]>();
    const inspection = deferred<ProjectDto>();
    const enrichment = deferred<AgentTabDto>();
    localStorage.setItem(`${SCOPE_KEY}.codex`, rememberedPath);
    state.projectsHandler = (agentId) =>
      agentId === "codex" ? projects.promise : Promise.resolve([]);
    state.inspectHandler = () => inspection.promise;
    state.refreshHandler = (agentId) =>
      agentId === "codex" ? enrichment.promise : Promise.resolve(EMPTY_TAB);
    renderSession();

    await waitFor(() => expect(state.events).toContain(`local:codex:${rememberedPath}`));
    expect(state.events).toContain(`inspect:codex:${rememberedPath}`);
    expect(screen.getByTestId("ready")).toHaveTextContent("no");

    inspection.resolve({ id: rememberedPath, label: "Remembered project", path: rememberedPath });

    await waitFor(() => expect(screen.getByTestId("ready")).toHaveTextContent("yes"));
    expect(screen.getByTestId("scope-label")).toHaveTextContent("Remembered project");
    expect(screen.getByTestId("plugin-id")).toHaveTextContent("workbench@workshop");
    expect(performance.mark).toHaveBeenCalledWith("on-n-off:remembered-scope-ready");

    projects.resolve([]);
    enrichment.resolve(ENRICHED_TAB);
  });

  it.each(["resolve", "reject"] as const)(
    "ignores an older project scan that finishes after a newer scan (%s)",
    async (lateResult) => {
      const firstProjects = deferred<ProjectDto[]>();
      const secondProjects = deferred<ProjectDto[]>();
      let codexProjectCalls = 0;
      state.projectsHandler = (agentId) => {
        if (agentId !== "codex") {
          return Promise.resolve([]);
        }
        codexProjectCalls += 1;
        return codexProjectCalls === 1 ? firstProjects.promise : secondProjects.promise;
      };
      renderSession();

      await waitFor(() => expect(screen.getByTestId("ready")).toHaveTextContent("yes"));
      fireEvent.click(screen.getByRole("button", { name: "Refresh Codex" }));
      await waitFor(() => expect(codexProjectCalls).toBe(2));

      secondProjects.resolve([{ id: "new-project", label: "New project", path: "C:/new" }]);
      await waitFor(() => expect(screen.getByTestId("projects")).toHaveTextContent("new-project"));

      await act(async () => {
        if (lateResult === "resolve") {
          firstProjects.resolve([{ id: "old-project", label: "Old project", path: "C:/old" }]);
        } else {
          firstProjects.reject(new Error("old scan failed"));
        }
        await firstProjects.promise.catch(() => undefined);
      });

      await waitFor(() => expect(screen.getByTestId("projects")).toHaveTextContent("new-project"));
      expect(screen.getByTestId("projects")).not.toHaveTextContent("old-project");
    },
  );

  it("coalesces a scope change during local loading and publishes only the new scope", async () => {
    const firstLocal = deferred<AgentTabDto>();
    const scopedTab: AgentTabDto = {
      ...EMPTY_TAB,
      plugins: [{ ...LOCAL_TAB.plugins[0], id: "new-scope-plugin", name: "new-scope-plugin" }],
    };
    let codexLocalCalls = 0;
    state.localHandler = (agentId, path) => {
      if (agentId !== "codex") {
        return Promise.resolve(EMPTY_TAB);
      }
      codexLocalCalls += 1;
      if (codexLocalCalls === 1) {
        return firstLocal.promise;
      }
      return Promise.resolve(path === "C:/fixture/new-scope" ? scopedTab : LOCAL_TAB);
    };
    renderSession();

    await waitFor(() => expect(state.events).toContain("local:codex:global"));
    expect(screen.getByTestId("busy")).toHaveTextContent("yes");
    fireEvent.click(screen.getByRole("button", { name: "Select new scope" }));
    firstLocal.resolve(LOCAL_TAB);

    await waitFor(() => expect(state.events).toContain("local:codex:C:/fixture/new-scope"));
    await waitFor(() => expect(screen.getByTestId("plugin-id")).toHaveTextContent("new-scope-plugin"));
    expect(screen.getByTestId("scope-path")).toHaveTextContent("C:/fixture/new-scope");
    expect(screen.getByTestId("plugin-id")).not.toHaveTextContent("workbench@workshop");
  });

  it("drains a scoped explicit refresh queued behind a failed mutation", async () => {
    const mutation = deferred<AgentTabDto>();
    state.mutationHandler = (agentId) =>
      agentId === "codex" ? mutation.promise : Promise.resolve(EMPTY_TAB);
    renderSession();
    await waitFor(() => expect(screen.getByTestId("ready")).toHaveTextContent("yes"));

    fireEvent.click(screen.getByRole("button", { name: "Mutate Codex" }));
    await waitFor(() => expect(state.events).toContain("mutate:codex"));
    expect(screen.getByTestId("busy")).toHaveTextContent("yes");
    fireEvent.click(screen.getByRole("button", { name: "Select new scope" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh Codex" }));
    expect(state.events).not.toContain("refresh:codex:C:/fixture/new-scope");

    mutation.reject(new Error("mutation failed"));

    await waitFor(() =>
      expect(state.events).toContain("refresh:codex:C:/fixture/new-scope"),
    );
    await waitFor(() => expect(screen.getByTestId("busy")).toHaveTextContent("no"));
    expect(screen.getByTestId("scope-path")).toHaveTextContent("C:/fixture/new-scope");
    expect(screen.getByTestId("banner")).toBeEmptyDOMElement();
  });
});
