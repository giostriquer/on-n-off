import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "$lib/api";
import {
  agentRoot,
  emptyTabDto,
  type LiveRow,
  type Screen,
} from "$lib/catalog";
import { copy } from "$lib/copy";
import { displayError, parseInvokeError } from "$lib/error";
import { flagOn, mergeFlags } from "$lib/flags";
import { ALL_AGENTS, DEFAULT_APP_SETTINGS, mergeAppSettings, setAgentHidden, visibleAgentIds } from "$lib/appSettings";
import type { FilteredTab } from "$lib/filterTab";
import { mergeProjects, projectFromPath, projectLabel, sameProjectPath } from "$lib/project";
import { markStartup } from "$lib/startupTiming";
import {
  LOCKED_AGENTS,
  emptyAgentRecord,
  emptyTab,
  mergeEnrichedPluginMetadata,
  openIds,
  overlayAgents,
  type TabState,
} from "$lib/session";
import { prependTrip, type TripEntry } from "$lib/tripLog";
import { applyTheme, readTheme, type Theme } from "$lib/theme";
import { installOutcomeClean, summarizeOutcomes } from "$lib/itemOutcomes";
import type {
  AgentId,
  AgentInfo,
  AgentTabDto,
  AppSettings,
  FeatureFlags,
  InstallItemsRequest,
  InstallItemsResult,
  McpServerDto,
  PluginDto,
  ProjectDto,
  SkillDto,
} from "$lib/types";
import { deriveCatalogFilterView, deriveCatalogInventory, deriveProjectView } from "./sessionView";

export const AGENT_KEY = "on-n-off.agent";
export const SCREEN_KEY = "on-n-off.screen";
export const SCOPE_KEY = "on-n-off.scope";

export { readTheme, THEME_KEY, type Theme } from "$lib/theme";

export const SCREEN_PATH: Record<Screen, string> = {
  overview: "/overview",
  plugins: "/plugins",
  skills: "/skills",
  mcp: "/mcp",
  usage: "/usage",
  limits: "/limits",
  config: "/config",
  settings: "/settings",
};

export function pathToScreen(pathname: string): Screen {
  const hit = (Object.entries(SCREEN_PATH) as [Screen, string][]).find(([, path]) => path === pathname);
  return hit?.[0] ?? "overview";
}

export function readAgent(): AgentId {
  const value = localStorage.getItem(AGENT_KEY);
  if (value === "codex" || value === "antigravity" || value === "cursor") {
    return value;
  }
  return "claude";
}

export function readScreen(): Screen {
  const value = localStorage.getItem(SCREEN_KEY);
  if (
    value === "plugins" ||
    value === "skills" ||
    value === "mcp" ||
    value === "usage" ||
    value === "limits" ||
    value === "config" ||
    value === "settings" ||
    value === "overview"
  ) {
    return value;
  }
  return "overview";
}

function readScope(agentId: AgentId): string | null {
  const value = localStorage.getItem(`${SCOPE_KEY}.${agentId}`)?.trim();
  return value || null;
}

function emptyTabs(): Record<AgentId, TabState> {
  return emptyAgentRecord(emptyTab);
}

type SessionContextValue = {
  theme: Theme;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;
  agents: AgentInfo[];
  selected: AgentId;
  setSelected: (id: AgentId) => void;
  tabs: Record<AgentId, TabState>;
  setFilter: (value: string) => void;
  log: TripEntry[];
  flags: FeatureFlags;
  appSettings: AppSettings;
  initialProviderReady: boolean;
  visibleAgents: AgentInfo[];
  persistAppSettings: (next: AppSettings) => Promise<AppSettings | null>;
  setProviderHidden: (id: AgentId, hidden: boolean) => Promise<void>;
  setProviderBinary: (id: AgentId, path: string) => Promise<void>;
  reloadAgents: () => Promise<void>;
  installOpen: boolean;
  setInstallOpen: (open: boolean) => void;
  installError: string | null;
  clearInstallError: () => void;
  uninstallTarget: PluginDto | null;
  setUninstallTarget: (plugin: PluginDto | null) => void;
  currentAgent: AgentInfo;
  currentTab: TabState;
  filtered: FilteredTab | null;
  expandedIds: Set<string>;
  banner: string | null;
  canInstall: boolean;
  counts: ReturnType<typeof deriveCatalogInventory>["counts"];
  live: LiveRow[];
  drift: ReturnType<typeof deriveCatalogInventory>["drift"];
  allOn: boolean;
  cliLine: string;
  masterNote: string;
  showMasterCut: boolean;
  currentProjects: ProjectDto[];
  currentScopePath: string | null;
  currentScopeLabel: string;
  scopeNote: string;
  loadTab: (agentId: AgentId, probe?: boolean) => Promise<void>;
  selectScope: (path: string | null) => Promise<void>;
  pickProjectFolder: () => Promise<void>;
  openProjectPath: (path: string) => Promise<void>;
  toggleExpand: (pluginId: string) => void;
  togglePlugin: (plugin: PluginDto, enabled: boolean) => void;
  toggleSkill: (skill: SkillDto, enabled: boolean) => void;
  toggleMcp: (server: McpServerDto, enabled: boolean) => void;
  toggleLive: (row: LiveRow, enabled: boolean) => void;
  masterCut: (enabled: boolean) => Promise<void>;
  pickFolder: () => Promise<string | null>;
  install: (source: string) => Promise<boolean>;
  installItems: (request: InstallItemsRequest) => Promise<InstallItemsResult | null>;
  installBusy: boolean;
  updatePlugin: (plugin: PluginDto) => void;
  confirmUninstall: () => Promise<void>;
  emptyTabDto: typeof emptyTabDto;
};

const SessionContext = createContext<SessionContextValue | null>(null);

export function useAgentSession(): SessionContextValue {
  const ctx = useContext(SessionContext);
  if (!ctx) {
    throw new Error("useAgentSession must be used within SessionProvider");
  }
  return ctx;
}

function bannerMessage(agent: AgentInfo, tabError: string | null): string | null {
  if (tabError) {
    return tabError;
  }
  if (!agent.cliOk) {
    return agent.cliError || copy.cliMissing(agent.displayName);
  }
  return null;
}

function patchTab(
  tabs: Record<AgentId, TabState>,
  agentId: AgentId,
  patch: Partial<TabState>,
): Record<AgentId, TabState> {
  return {
    ...tabs,
    [agentId]: { ...tabs[agentId], ...patch },
  };
}

export function SessionProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [agents, setAgents] = useState<AgentInfo[]>(() => LOCKED_AGENTS.map((agent) => ({ ...agent })));
  const [selected, setSelectedState] = useState<AgentId>(readAgent);
  const [tabs, setTabs] = useState<Record<AgentId, TabState>>(emptyTabs);
  const [log, setLog] = useState<TripEntry[]>([]);
  const [flags, setFlags] = useState<FeatureFlags>(() => mergeFlags(null));
  const [appSettings, setAppSettings] = useState<AppSettings>(DEFAULT_APP_SETTINGS);
  const [initialProviderReady, setInitialProviderReady] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [installBusy, setInstallBusy] = useState(false);
  const [uninstallTarget, setUninstallTarget] = useState<PluginDto | null>(null);
  const [projects, setProjects] = useState<Record<AgentId, ProjectDto[]>>(() => emptyAgentRecord(() => []));
  const [extraProjects, setExtraProjects] = useState<Record<AgentId, ProjectDto[]>>(() =>
    emptyAgentRecord(() => []),
  );
  const [selectedScope, setSelectedScope] = useState<Record<AgentId, string | null>>({
    claude: readScope("claude"),
    codex: readScope("codex"),
    antigravity: readScope("antigravity"),
    cursor: readScope("cursor"),
  });

  const tabsRef = useRef(tabs);
  const agentsRef = useRef(agents);
  const selectedRef = useRef(selected);
  const selectedScopeRef = useRef(selectedScope);
  const projectsRef = useRef(projects);
  const extraProjectsRef = useRef(extraProjects);
  const inFlightRef = useRef<Record<AgentId, boolean>>(emptyAgentRecord(() => false));
  const loadGenerationRef = useRef<Record<AgentId, number>>(emptyAgentRecord(() => 0));
  const pendingLoadRef = useRef<Record<AgentId, boolean | null>>(emptyAgentRecord(() => null));
  const loadLoopRef = useRef<Record<AgentId, boolean>>(emptyAgentRecord(() => false));
  const loadTabRef = useRef<(agentId: AgentId, probe?: boolean) => Promise<void>>(async () => {});
  const bootStartedRef = useRef(false);

  tabsRef.current = tabs;
  agentsRef.current = agents;
  selectedRef.current = selected;
  selectedScopeRef.current = selectedScope;
  projectsRef.current = projects;
  extraProjectsRef.current = extraProjects;

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    localStorage.setItem(AGENT_KEY, selected);
  }, [selected]);

  const note = useCallback((tag: TripEntry["tag"], text: string) => {
    setLog((prev) => prependTrip(prev, tag, text));
  }, []);

  const agentLabel = useCallback((agentId: AgentId): string => {
    return agentsRef.current.find((agent) => agent.id === agentId)?.displayName ?? agentId;
  }, []);

  const scopeSuffix = useCallback((agentId: AgentId = selectedRef.current): string => {
    const path = selectedScopeRef.current[agentId];
    if (!path) {
      return "all projects";
    }
    return (
      mergeProjects(projectsRef.current[agentId], extraProjectsRef.current[agentId]).find((project) =>
        sameProjectPath(project.path, path),
      )?.label ?? projectLabel(path)
    );
  }, []);

  const rememberExtra = useCallback(async (agentId: AgentId, path: string) => {
    if (
      mergeProjects(projectsRef.current[agentId], extraProjectsRef.current[agentId]).some((project) =>
        sameProjectPath(project.path, path),
      )
    ) {
      return;
    }
    try {
      const inspected = await api.inspectProject(agentId, path);
      setExtraProjects((prev) => ({ ...prev, [agentId]: [...prev[agentId], inspected] }));
    } catch {
      setExtraProjects((prev) => ({ ...prev, [agentId]: [...prev[agentId], projectFromPath(path)] }));
    }
  }, []);

  const loadProjects = useCallback(
    async (agentId: AgentId, generation: number) => {
      try {
        const next = await api.listProjects(agentId);
        if (loadGenerationRef.current[agentId] !== generation) {
          return;
        }
        setProjects((prev) => ({ ...prev, [agentId]: next }));
      } catch {
        if (loadGenerationRef.current[agentId] !== generation) {
          return;
        }
        setProjects((prev) => ({ ...prev, [agentId]: [] }));
      }
    },
    [],
  );

  const loadRememberedScope = useCallback(
    async (agentId: AgentId) => {
      const saved = selectedScopeRef.current[agentId];
      if (saved) {
        await rememberExtra(agentId, saved);
        if (agentId === selectedRef.current) {
          markStartup("remembered-scope-ready");
        }
      }
    },
    [rememberExtra],
  );

  const applyScopedTab = useCallback(async (agentId: AgentId, fallback: AgentTabDto) => {
    try {
      const next = await api.listPlugins(agentId, selectedScopeRef.current[agentId]);
      setTabs((prev) => patchTab(prev, agentId, { dto: next }));
    } catch {
      setTabs((prev) => patchTab(prev, agentId, { dto: fallback }));
    }
  }, []);

  const withLock = useCallback(async (agentId: AgentId, fn: () => Promise<void>) => {
    if (inFlightRef.current[agentId]) {
      return;
    }
    inFlightRef.current[agentId] = true;
    setTabs((prev) => patchTab(prev, agentId, { inFlight: true }));
    try {
      await fn();
    } finally {
      inFlightRef.current[agentId] = false;
      setTabs((prev) => patchTab(prev, agentId, { inFlight: false }));
      const pendingProbe = pendingLoadRef.current[agentId];
      if (!loadLoopRef.current[agentId] && pendingProbe !== null) {
        pendingLoadRef.current[agentId] = null;
        void loadTabRef.current(agentId, pendingProbe);
      }
    }
  }, []);

  const enrichTab = useCallback(
    async (agentId: AgentId, projectPath: string | null, generation: number) => {
      try {
        const enriched = await api.refresh(agentId, projectPath);
        if (loadGenerationRef.current[agentId] !== generation) {
          return;
        }
        setTabs((prev) => {
          const local = prev[agentId].dto;
          return patchTab(prev, agentId, {
            dto: local ? mergeEnrichedPluginMetadata(local, enriched) : enriched,
          });
        });
      } catch (error) {
        if (loadGenerationRef.current[agentId] !== generation) {
          return;
        }
        const message = displayError(parseInvokeError(error), agentLabel(agentId));
        setTabs((prev) => patchTab(prev, agentId, { error: message }));
        note("TRIP", `${agentLabel(agentId)} enrichment failed`);
      }
    },
    [agentLabel, note],
  );

  const loadTab = useCallback(
    async (agentId: AgentId, probe = false) => {
      if (inFlightRef.current[agentId]) {
        loadGenerationRef.current[agentId] += 1;
        pendingLoadRef.current[agentId] = pendingLoadRef.current[agentId] === true || probe;
        return;
      }
      loadLoopRef.current[agentId] = true;
      try {
        let requestedProbe = probe;
        while (true) {
          pendingLoadRef.current[agentId] = null;
          const generation = loadGenerationRef.current[agentId] + 1;
          loadGenerationRef.current[agentId] = generation;
          const projectPath = selectedScopeRef.current[agentId];
          let localReady = false;
          await withLock(agentId, async () => {
            if (requestedProbe) {
              try {
                setAgents(overlayAgents(await api.listAgents()));
              } catch {
                setAgents(overlayAgents([]));
              }
            }
            setTabs((prev) => patchTab(prev, agentId, { loading: true }));
            const projects = loadProjects(agentId, generation);
            const rememberedScope = loadRememberedScope(agentId);
            try {
              const dto = requestedProbe
                ? await api.refresh(agentId, projectPath)
                : await api.listLocalPlugins(agentId, projectPath);
              await rememberedScope;
              if (requestedProbe) {
                await projects;
              }
              if (loadGenerationRef.current[agentId] !== generation) {
                return;
              }
              setTabs((prev) => patchTab(prev, agentId, { dto, error: null }));
              const itemCount = (dto?.plugins.length ?? 0) + (dto?.userSkills.length ?? 0);
              if (requestedProbe || agentId === selectedRef.current) {
                note("SYNC", `scanned ${agentLabel(agentId)} config · ${itemCount} items`);
              }
              localReady = !requestedProbe;
            } catch (error) {
              if (loadGenerationRef.current[agentId] !== generation) {
                return;
              }
              setTabs((prev) =>
                patchTab(prev, agentId, {
                  dto: null,
                  error: displayError(parseInvokeError(error), agentLabel(agentId)),
                }),
              );
              note("TRIP", `${agentLabel(agentId)} refresh failed`);
            } finally {
              if (loadGenerationRef.current[agentId] === generation) {
                setTabs((prev) => patchTab(prev, agentId, { loading: false }));
              }
            }
          });
          if (localReady && loadGenerationRef.current[agentId] === generation) {
            void enrichTab(agentId, projectPath, generation);
          }
          const pendingProbe = pendingLoadRef.current[agentId];
          if (pendingProbe === null) {
            return;
          }
          requestedProbe = pendingProbe;
        }
      } finally {
        loadLoopRef.current[agentId] = false;
      }
    },
    [agentLabel, enrichTab, loadProjects, loadRememberedScope, note, withLock],
  );
  loadTabRef.current = loadTab;

  useEffect(() => {
    if (bootStartedRef.current) {
      return;
    }
    bootStartedRef.current = true;
    void (async () => {
      const [nextFlags, nextSettings, nextAgents] = await Promise.allSettled([
        api.featureFlags(),
        api.loadAppSettings(),
        api.listAgents(),
      ]);
      setFlags(mergeFlags(nextFlags.status === "fulfilled" ? nextFlags.value : null));
      const settings =
        nextSettings.status === "fulfilled" ? mergeAppSettings(nextSettings.value) : DEFAULT_APP_SETTINGS;
      setAppSettings(settings);
      // The remembered provider may have been hidden since (Settings → Providers): boot on the
      // first visible one instead of loading a tab the user cannot reach.
      const visible = visibleAgentIds(settings.hiddenAgents);
      if (!visible.includes(selectedRef.current) && visible[0]) {
        selectedRef.current = visible[0];
        setSelectedState(visible[0]);
      }
      if (nextAgents.status === "fulfilled") {
        setAgents(overlayAgents(nextAgents.value));
      } else {
        setAgents(overlayAgents([]));
        const agent = agentsRef.current.find((a) => a.id === selectedRef.current) ?? agentsRef.current[0];
        setTabs((prev) =>
          patchTab(prev, selectedRef.current, {
            error: displayError(parseInvokeError(nextAgents.reason), agent.displayName),
          }),
        );
      }
      const primary = selectedRef.current;
      await loadTab(primary);
      markStartup("selected-local-ready");
      setInitialProviderReady(true);
      const background = ALL_AGENTS.filter((id) => id !== primary);
      markStartup("background-providers-start");
      void Promise.all(background.map((id) => loadTab(id)));
    })();
  }, [loadTab]);

  const setSelected = useCallback(
    (id: AgentId) => {
      setSelectedState(id);
      if (!tabsRef.current[id].dto && !inFlightRef.current[id]) {
        void loadTab(id);
      }
    },
    [loadTab],
  );

  const setFilter = useCallback(
    (value: string) => {
      setTabs((prev) => patchTab(prev, selected, { filter: value }));
    },
    [selected],
  );

  const selectScope = useCallback(
    async (path: string | null) => {
      const agentId = selectedRef.current;
      selectedScopeRef.current = { ...selectedScopeRef.current, [agentId]: path };
      setSelectedScope(selectedScopeRef.current);
      localStorage.setItem(`${SCOPE_KEY}.${agentId}`, path ?? "");
      await loadTab(agentId);
    },
    [loadTab],
  );

  const pickProjectFolder = useCallback(async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") {
      return;
    }
    await rememberExtra(selectedRef.current, dir);
    await selectScope(dir);
  }, [rememberExtra, selectScope]);

  const openProjectPath = useCallback(
    async (path: string) => {
      await rememberExtra(selectedRef.current, path);
      await selectScope(path);
    },
    [rememberExtra, selectScope],
  );

  const toggleExpand = useCallback(
    (pluginId: string) => {
      setTabs((prev) => {
        const next = new Set(prev[selected].expanded);
        if (next.has(pluginId)) {
          next.delete(pluginId);
        } else {
          next.add(pluginId);
        }
        return patchTab(prev, selected, { expanded: next });
      });
    },
    [selected],
  );

  const mutate = useCallback(
    async (fn: () => Promise<AgentTabDto>, okNote?: { tag: TripEntry["tag"]; text: string }) => {
      const agentId = selectedRef.current;
      await withLock(agentId, async () => {
        try {
          const next = await fn();
          await applyScopedTab(agentId, next);
          setTabs((prev) => patchTab(prev, agentId, { error: null }));
          if (okNote) {
            note(okNote.tag, okNote.text);
          }
        } catch (error) {
          const message = displayError(parseInvokeError(error), agentLabel(agentId));
          setTabs((prev) => patchTab(prev, agentId, { error: message }));
          note("TRIP", message);
        }
      });
    },
    [agentLabel, applyScopedTab, note, withLock],
  );

  const togglePlugin = useCallback(
    (plugin: PluginDto, enabled: boolean) => {
      const agent = agentsRef.current.find((a) => a.id === selectedRef.current) ?? agentsRef.current[0];
      void mutate(() => api.setPluginEnabled(selectedRef.current, plugin.id, enabled), {
        tag: enabled ? "ON" : "OFF",
        text: `${plugin.name} ${enabled ? "enabled" : "cut"} for ${agent.displayName} · ${scopeSuffix()}`,
      });
    },
    [mutate, scopeSuffix],
  );

  const toggleSkill = useCallback(
    (skill: SkillDto, enabled: boolean) => {
      const agent = agentsRef.current.find((a) => a.id === selectedRef.current) ?? agentsRef.current[0];
      void mutate(() => api.setSkillEnabled(selectedRef.current, skill.id, enabled), {
        tag: enabled ? "ON" : "OFF",
        text: `${skill.name} ${enabled ? "enabled" : "cut"} for ${agent.displayName} · ${scopeSuffix()}`,
      });
    },
    [mutate, scopeSuffix],
  );

  const toggleMcp = useCallback(
    (server: McpServerDto, enabled: boolean) => {
      const agent = agentsRef.current.find((a) => a.id === selectedRef.current) ?? agentsRef.current[0];
      void mutate(() => api.setMcpEnabled(selectedRef.current, server.id, enabled), {
        tag: enabled ? "ON" : "OFF",
        text: `${server.name} ${enabled ? "enabled" : "cut"} for ${agent.displayName} · ${scopeSuffix()}`,
      });
    },
    [mutate, scopeSuffix],
  );

  const toggleLive = useCallback(
    (row: LiveRow, enabled: boolean) => {
      const dto = tabsRef.current[selectedRef.current].dto;
      if (row.kind === "plugin") {
        const plugin = dto?.plugins.find((item) => item.id === row.id);
        if (plugin) {
          togglePlugin(plugin, enabled);
        }
        return;
      }
      if (row.kind === "mcp") {
        const server = dto?.mcpServers.find((item) => item.id === row.id);
        if (server?.togglable) {
          toggleMcp(server, enabled);
        }
        return;
      }
      const skill =
        dto?.userSkills.find((item) => item.id === row.id) ??
        dto?.plugins.flatMap((plugin) => plugin.skills).find((item) => item.id === row.id);
      if (skill?.togglable) {
        toggleSkill(skill, enabled);
      }
    },
    [toggleMcp, togglePlugin, toggleSkill],
  );

  const masterCut = useCallback(
    async (enabled: boolean) => {
      if (!flagOn(flags, "masterCut")) {
        return;
      }
      const agentId = selectedRef.current;
      const dto = tabsRef.current[agentId].dto;
      if (!dto) {
        return;
      }
      await withLock(agentId, async () => {
        let current = dto;
        try {
          for (const plugin of current.plugins) {
            if (plugin.togglable && plugin.enabled !== enabled) {
              current = await api.setPluginEnabled(agentId, plugin.id, enabled);
            }
          }
          for (const skill of current.userSkills) {
            if (skill.togglable && skill.enabled !== enabled) {
              current = await api.setSkillEnabled(agentId, skill.id, enabled);
            }
          }
          for (const server of current.mcpServers) {
            if (server.togglable && server.enabled !== enabled) {
              current = await api.setMcpEnabled(agentId, server.id, enabled);
            }
          }
          await applyScopedTab(agentId, current);
          setTabs((prev) => patchTab(prev, agentId, { error: null }));
          note(
            enabled ? "ON" : "TRIP",
            `master ${enabled ? "restored" : "cut"} for ${agentLabel(agentId)} · ${scopeSuffix(agentId)}`,
          );
        } catch (error) {
          setTabs((prev) =>
            patchTab(prev, agentId, {
              dto: current,
              error: displayError(parseInvokeError(error), agentLabel(agentId)),
            }),
          );
          note("TRIP", `master cut failed for ${agentLabel(agentId)}`);
        }
      });
    },
    [agentLabel, applyScopedTab, flags, note, scopeSuffix, withLock],
  );

  const pickFolder = useCallback(async (): Promise<string | null> => {
    const dir = await open({ directory: true, multiple: false });
    return typeof dir === "string" ? dir : null;
  }, []);

  const install = useCallback(
    async (source: string): Promise<boolean> => {
      setInstallError(null);
      const agentId = selectedRef.current;
      let ok = false;
      await withLock(agentId, async () => {
        try {
          const next = await api.installPlugin(agentId, source);
          await applyScopedTab(agentId, next);
          setTabs((prev) => patchTab(prev, agentId, { error: null }));
          setInstallOpen(false);
          note("INST", `${source} installed · enabled on ${agentLabel(agentId)}`);
          ok = true;
        } catch (error) {
          const message = displayError(parseInvokeError(error), agentLabel(agentId));
          setInstallError(message);
          note("TRIP", message);
        }
      });
      return ok;
    },
    [agentLabel, applyScopedTab, note, withLock],
  );

  const installItems = useCallback(
    async (request: InstallItemsRequest): Promise<InstallItemsResult | null> => {
      setInstallError(null);
      setInstallBusy(true);
      try {
        const result = await api.installItems(request);
        const summary = summarizeOutcomes(result);
        // loadTab queues a reload when a provider is mid-flight; withLock would drop it.
        for (const provider of summary.touchedProviders) {
          void loadTabRef.current(provider);
        }
        if (summary.installed > 0) {
          note(
            "INST",
            `${summary.installed} item${summary.installed === 1 ? "" : "s"} from ${request.source.owner}/${request.source.repo} · ${summary.touchedProviders.map(agentLabel).join(", ")}`,
          );
        }
        if (installOutcomeClean(result)) {
          setInstallOpen(false);
        } else {
          const pending = summary.conflicts.length + summary.failed.length;
          note("TRIP", `${pending} item${pending === 1 ? "" : "s"} need attention (conflict or failure)`);
        }
        return result;
      } catch (error) {
        const message = displayError(parseInvokeError(error), agentLabel(selectedRef.current));
        setInstallError(message);
        note("TRIP", message);
        return null;
      } finally {
        setInstallBusy(false);
      }
    },
    [agentLabel, note],
  );

  const updatePlugin = useCallback(
    (plugin: PluginDto) => {
      const agent = agentsRef.current.find((a) => a.id === selectedRef.current) ?? agentsRef.current[0];
      void mutate(() => api.updatePlugin(selectedRef.current, plugin.id), {
        tag: "SYNC",
        text: `${plugin.name} updated on ${agent.displayName}`,
      });
    },
    [mutate],
  );

  const confirmUninstall = useCallback(async () => {
    if (!uninstallTarget) {
      return;
    }
    const target = uninstallTarget;
    await mutate(() => api.uninstallPlugin(selectedRef.current, target.id), {
      tag: "TRIP",
      text: `${target.name} uninstalled from ${agentLabel(selectedRef.current)}`,
    });
    setUninstallTarget(null);
  }, [agentLabel, mutate, uninstallTarget]);

  const reloadAgents = useCallback(async () => {
    try {
      setAgents(overlayAgents(await api.listAgents()));
    } catch {
      setAgents(overlayAgents([]));
    }
  }, []);

  const persistAppSettings = useCallback(
    async (next: AppSettings) => {
      try {
        const saved = mergeAppSettings(await api.saveAppSettings(next));
        setAppSettings(saved);
        const visible = visibleAgentIds(saved.hiddenAgents);
        if (!visible.includes(selectedRef.current) && visible[0]) {
          setSelectedState(visible[0]);
        }
        return saved;
      } catch (error) {
        note("TRIP", displayError(parseInvokeError(error), "Settings"));
        return null;
      }
    },
    [note],
  );

  const setProviderHidden = useCallback(
    async (id: AgentId, hidden: boolean) => {
      const nextHidden = setAgentHidden(appSettings.hiddenAgents, id, hidden);
      await persistAppSettings({ ...appSettings, hiddenAgents: nextHidden });
    },
    [appSettings, persistAppSettings],
  );

  const setProviderBinary = useCallback(
    async (id: AgentId, path: string) => {
      const binaryPaths = { ...appSettings.binaryPaths };
      if (path.trim()) {
        binaryPaths[id] = path.trim();
      } else {
        delete binaryPaths[id];
      }
      const saved = await persistAppSettings({ ...appSettings, binaryPaths });
      if (saved) {
        await reloadAgents();
      }
    },
    [appSettings, persistAppSettings, reloadAgents],
  );

  const currentAgent = useMemo(
    () => agents.find((agent) => agent.id === selected) ?? agents[0],
    [agents, selected],
  );
  const visibleAgents = useMemo(() => {
    const visible = visibleAgentIds(appSettings.hiddenAgents);
    return agents.filter((agent) => visible.includes(agent.id));
  }, [agents, appSettings.hiddenAgents]);
  const currentTab = tabs[selected];
  const catalogInventory = useMemo(
    () => deriveCatalogInventory(currentTab.dto),
    [currentTab.dto],
  );
  const catalogFilterView = useMemo(
    () => deriveCatalogFilterView(currentTab.dto, currentTab.filter, catalogInventory.live),
    [catalogInventory.live, currentTab.dto, currentTab.filter],
  );
  const filtered = catalogFilterView.filtered;
  const expandedIds = useMemo(
    () => openIds(currentTab.expanded, filtered?.expandIds ?? []),
    [currentTab.expanded, filtered],
  );
  const banner = bannerMessage(currentAgent, currentTab.error);
  const canInstall = currentAgent.cliOk && currentAgent.installGit && !currentTab.inFlight;
  const { counts, drift, allOn } = catalogInventory;
  const live = catalogFilterView.live;
  const cliLine = currentAgent.cliOk
    ? `${currentAgent.id} · ${agentRoot(currentAgent.id)}`
    : `${currentAgent.id} · offline`;
  const masterNote = allOn
    ? `everything live on ${currentAgent.displayName}`
    : `cuts every item for ${currentAgent.displayName}`;
  const showMasterCut = flagOn(flags, "masterCut");
  const selectedProjects = projects[selected];
  const selectedExtraProjects = extraProjects[selected];
  const selectedScopePath = selectedScope[selected];
  const projectView = useMemo(
    () => deriveProjectView(selectedProjects, selectedExtraProjects, selectedScopePath),
    [selectedExtraProjects, selectedProjects, selectedScopePath],
  );
  const currentProjects = projectView.projects;
  const currentScopePath = projectView.path;
  const currentScopeLabel = projectView.label;
  const scopeNote = projectView.note;

  const value = useMemo<SessionContextValue>(
    () => ({
      theme,
      setTheme,
      toggleTheme: () => setTheme((prev) => (prev === "dark" ? "light" : "dark")),
      agents,
      visibleAgents,
      selected,
      setSelected,
      tabs,
      setFilter,
      log,
      flags,
      appSettings,
      initialProviderReady,
      persistAppSettings,
      setProviderHidden,
      setProviderBinary,
      reloadAgents,
      installOpen,
      setInstallOpen,
      installError,
      clearInstallError: () => setInstallError(null),
      uninstallTarget,
      setUninstallTarget,
      currentAgent,
      currentTab,
      filtered,
      expandedIds,
      banner,
      canInstall,
      counts,
      live,
      drift,
      allOn,
      cliLine,
      masterNote,
      showMasterCut,
      currentProjects,
      currentScopePath,
      currentScopeLabel,
      scopeNote,
      loadTab,
      selectScope,
      pickProjectFolder,
      openProjectPath,
      toggleExpand,
      togglePlugin,
      toggleSkill,
      toggleMcp,
      toggleLive,
      masterCut,
      pickFolder,
      install,
      installItems,
      installBusy,
      updatePlugin,
      confirmUninstall,
      emptyTabDto,
    }),
    [
      theme,
      setTheme,
      agents,
      visibleAgents,
      selected,
      setSelected,
      tabs,
      setFilter,
      log,
      flags,
      appSettings,
      initialProviderReady,
      persistAppSettings,
      setProviderHidden,
      setProviderBinary,
      reloadAgents,
      installOpen,
      installError,
      uninstallTarget,
      currentAgent,
      currentTab,
      filtered,
      expandedIds,
      banner,
      canInstall,
      counts,
      live,
      drift,
      allOn,
      cliLine,
      masterNote,
      showMasterCut,
      currentProjects,
      currentScopePath,
      currentScopeLabel,
      scopeNote,
      loadTab,
      selectScope,
      pickProjectFolder,
      openProjectPath,
      toggleExpand,
      togglePlugin,
      toggleSkill,
      toggleMcp,
      toggleLive,
      masterCut,
      pickFolder,
      install,
      installItems,
      installBusy,
      updatePlugin,
      confirmUninstall,
    ],
  );

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}
