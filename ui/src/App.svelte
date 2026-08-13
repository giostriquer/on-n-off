<script lang="ts">
  import { onMount } from "svelte";
  import RefreshCw from "lucide-svelte/icons/refresh-cw";
  import AgentBanner from "$lib/AgentBanner.svelte";
  import AgentConfig from "$lib/AgentConfig.svelte";
  import ConfirmDialog from "$lib/ConfirmDialog.svelte";
  import InstallSheet from "$lib/InstallSheet.svelte";
  import ItemList from "$lib/ItemList.svelte";
  import LeftRail from "$lib/LeftRail.svelte";
  import McpList from "$lib/McpList.svelte";
  import Overview from "$lib/Overview.svelte";
  import Rocker from "$lib/Rocker.svelte";
  import ScopeBar from "$lib/ScopeBar.svelte";
  import Usage from "$lib/Usage.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "$lib/api";
  import {
    agentRoot,
    catalogCounts,
    driftRows,
    emptyTabDto,
    globalItemCount,
    liveRows,
    masterAllOn,
    tallyLine,
    type LiveRow,
    type Screen,
  } from "$lib/catalog";
  import { copy } from "$lib/copy";
  import { displayError, parseInvokeError } from "$lib/error";
  import { flagOn, mergeFlags } from "$lib/flags";
  import { filterTab } from "$lib/filterTab";
  import { mergeProjects, projectFromPath, projectLabel, sameProjectPath } from "$lib/project";
  import { LOCKED_AGENTS, emptyTab, openIds, overlayAgents, withAgentLock, type TabState } from "$lib/session";
  import { prependTrip, type TripEntry } from "$lib/tripLog";
  import type { AgentId, AgentInfo, AgentTabDto, FeatureFlags, McpServerDto, PluginDto, ProjectDto, SkillDto } from "$lib/types";

  const THEME_KEY = "on-n-off.theme";
  const AGENT_KEY = "on-n-off.agent";
  const SCREEN_KEY = "on-n-off.screen";
  const SCOPE_KEY = "on-n-off.scope";
  type Theme = "dark" | "light";

  let theme = $state<Theme>(readTheme());
  let screen = $state<Screen>(readScreen());
  let agents = $state<AgentInfo[]>(LOCKED_AGENTS.map((agent) => ({ ...agent })));
  let selected = $state<AgentId>(readAgent());
  let tabs = $state<Record<AgentId, TabState>>({
    claude: emptyTab(),
    codex: emptyTab(),
    antigravity: emptyTab(),
  });
  let log = $state<TripEntry[]>([]);
  let flags = $state<FeatureFlags>(mergeFlags(null));
  let installOpen = $state(false);
  let installError = $state<string | null>(null);
  let uninstallTarget = $state<PluginDto | null>(null);
  let projects = $state<Record<AgentId, ProjectDto[]>>({ claude: [], codex: [], antigravity: [] });
  let extraProjects = $state<Record<AgentId, ProjectDto[]>>({ claude: [], codex: [], antigravity: [] });
  let selectedScope = $state<Record<AgentId, string | null>>({
    claude: readScope("claude"),
    codex: readScope("codex"),
    antigravity: readScope("antigravity"),
  });
  let usageVisited = $state(screen === "usage");

  const usageScreen = $derived(screen === "usage");
  const currentAgent = $derived(agents.find((agent) => agent.id === selected) ?? agents[0]);
  const currentTab = $derived(tabs[selected]);
  const filtered = $derived(currentTab.dto ? filterTab(currentTab.dto, currentTab.filter) : null);
  const expandedIds = $derived(openIds(currentTab.expanded, filtered?.expandIds ?? []));
  const banner = $derived(bannerMessage(currentAgent, currentTab.error));
  const canInstall = $derived(currentAgent.cliOk && currentAgent.installGit && !currentTab.inFlight);
  const counts = $derived(catalogCounts(currentTab.dto));
  const live = $derived(
    currentTab.dto
      ? liveRows(currentTab.dto).filter((row) => {
          const q = currentTab.filter.trim().toLowerCase();
          return !q || `${row.name} ${row.meta} ${row.id}`.toLowerCase().includes(q);
        })
      : [],
  );
  const drift = $derived(currentTab.dto ? driftRows(currentTab.dto) : []);
  const allOn = $derived(masterAllOn(currentTab.dto));
  const cliLine = $derived(
    currentAgent.cliOk
      ? `${currentAgent.id} · ${agentRoot(currentAgent.id)}`
      : `${currentAgent.id} · offline`,
  );
  const masterNote = $derived(
    allOn ? `everything live on ${currentAgent.displayName}` : `cuts every item for ${currentAgent.displayName}`,
  );
  const showMasterCut = $derived(flagOn(flags, "masterCut"));
  const currentProjects = $derived(mergeProjects(projects[selected], extraProjects[selected]));
  const currentScopePath = $derived(selectedScope[selected]);
  const currentScopeLabel = $derived(
    currentScopePath
      ? (currentProjects.find((project) => sameProjectPath(project.path, currentScopePath))?.label ??
        projectLabel(currentScopePath))
      : "all projects",
  );
  const scopeNote = $derived(
    currentScopePath ? `local skills · ${currentScopePath}` : "global agent config is the source of truth",
  );

  $effect(() => {
    const root = document.documentElement;
    root.dataset.theme = "onnoff";
    root.classList.toggle("dark", theme === "dark");
    localStorage.setItem(THEME_KEY, theme);
  });

  $effect(() => {
    localStorage.setItem(AGENT_KEY, selected);
  });

  $effect(() => {
    localStorage.setItem(SCREEN_KEY, screen);
    if (screen === "usage") {
      usageVisited = true;
    }
  });

  onMount(() => {
    void boot();
  });

  function readTheme(): Theme {
    return localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
  }

  function readAgent(): AgentId {
    const value = localStorage.getItem(AGENT_KEY);
    if (value === "codex" || value === "antigravity") {
      return value;
    }
    return "claude";
  }

  function readScreen(): Screen {
    const value = localStorage.getItem(SCREEN_KEY);
    if (
      value === "plugins" ||
      value === "skills" ||
      value === "mcp" ||
      value === "usage" ||
      value === "config" ||
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

  function scopeSuffix(agentId: AgentId = selected): string {
    const path = selectedScope[agentId];
    if (!path) {
      return "all projects";
    }
    return (
      mergeProjects(projects[agentId], extraProjects[agentId]).find((project) => sameProjectPath(project.path, path))
        ?.label ?? projectLabel(path)
    );
  }

  async function rememberExtra(agentId: AgentId, path: string) {
    if (mergeProjects(projects[agentId], extraProjects[agentId]).some((project) => sameProjectPath(project.path, path))) {
      return;
    }
    try {
      extraProjects[agentId] = [...extraProjects[agentId], await api.inspectProject(agentId, path)];
    } catch {
      extraProjects[agentId] = [...extraProjects[agentId], projectFromPath(path)];
    }
  }

  function note(tag: TripEntry["tag"], text: string) {
    log = prependTrip(log, tag, text);
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

  async function boot() {
    try {
      flags = mergeFlags(await api.featureFlags());
    } catch {
      flags = mergeFlags(null);
    }
    try {
      agents = overlayAgents(await api.listAgents());
    } catch (error) {
      agents = overlayAgents([]);
      tabs[selected].error = displayError(parseInvokeError(error), currentAgent.displayName);
    }
    await Promise.all([loadTab("claude"), loadTab("codex"), loadTab("antigravity")]);
  }

  async function loadTab(agentId: AgentId, probe = false) {
    await withAgentLock(tabs, agentId, async () => {
      if (probe) {
        try {
          agents = overlayAgents(await api.listAgents());
        } catch {
          agents = overlayAgents([]);
        }
      }
      tabs[agentId].loading = true;
      try {
        await loadProjects(agentId);
        tabs[agentId].dto = await api.refresh(agentId, selectedScope[agentId]);
        tabs[agentId].error = null;
        const itemCount =
          (tabs[agentId].dto?.plugins.length ?? 0) + (tabs[agentId].dto?.userSkills.length ?? 0);
        if (probe || agentId === selected) {
          note("SYNC", `scanned ${agentLabel(agentId)} config · ${itemCount} items`);
        }
      } catch (error) {
        tabs[agentId].dto = null;
        tabs[agentId].error = displayError(parseInvokeError(error), agentLabel(agentId));
        note("TRIP", `${agentLabel(agentId)} refresh failed`);
      } finally {
        tabs[agentId].loading = false;
      }
    });
  }

  function agentLabel(agentId: AgentId): string {
    return agents.find((agent) => agent.id === agentId)?.displayName ?? agentId;
  }

  async function loadProjects(agentId: AgentId) {
    try {
      projects[agentId] = await api.listProjects(agentId);
    } catch {
      projects[agentId] = [];
    }
    const saved = selectedScope[agentId];
    if (saved) {
      await rememberExtra(agentId, saved);
    }
  }

  async function applyScopedTab(agentId: AgentId, fallback: AgentTabDto) {
    try {
      tabs[agentId].dto = await api.listPlugins(agentId, selectedScope[agentId]);
    } catch {
      tabs[agentId].dto = fallback;
    }
  }

  async function selectScope(path: string | null) {
    selectedScope[selected] = path;
    localStorage.setItem(`${SCOPE_KEY}.${selected}`, path ?? "");
    await loadTab(selected);
  }

  async function pickProjectFolder() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") {
      return;
    }
    await rememberExtra(selected, dir);
    await selectScope(dir);
  }

  async function openProjectPath(path: string) {
    await rememberExtra(selected, path);
    await selectScope(path);
  }

  function toggleExpand(pluginId: string) {
    const next = new Set(tabs[selected].expanded);
    if (next.has(pluginId)) {
      next.delete(pluginId);
    } else {
      next.add(pluginId);
    }
    tabs[selected].expanded = next;
  }

  async function mutate(fn: () => Promise<AgentTabDto>, okNote?: { tag: TripEntry["tag"]; text: string }) {
    const agentId = selected;
    await withAgentLock(tabs, agentId, async () => {
      try {
        const next = await fn();
        await applyScopedTab(agentId, next);
        tabs[agentId].error = null;
        if (okNote) {
          note(okNote.tag, okNote.text);
        }
      } catch (error) {
        tabs[agentId].error = displayError(parseInvokeError(error), agentLabel(agentId));
        note("TRIP", displayError(parseInvokeError(error), agentLabel(agentId)));
      }
    });
  }

  function togglePlugin(plugin: PluginDto, enabled: boolean) {
    void mutate(
      () => api.setPluginEnabled(selected, plugin.id, enabled),
      {
        tag: enabled ? "ON" : "OFF",
        text: `${plugin.name} ${enabled ? "enabled" : "cut"} for ${currentAgent.displayName} · ${scopeSuffix()}`,
      },
    );
  }

  function toggleSkill(skill: SkillDto, enabled: boolean) {
    void mutate(
      () => api.setSkillEnabled(selected, skill.id, enabled),
      {
        tag: enabled ? "ON" : "OFF",
        text: `${skill.name} ${enabled ? "enabled" : "cut"} for ${currentAgent.displayName} · ${scopeSuffix()}`,
      },
    );
  }

  function toggleMcp(server: McpServerDto, enabled: boolean) {
    void mutate(
      () => api.setMcpEnabled(selected, server.id, enabled),
      {
        tag: enabled ? "ON" : "OFF",
        text: `${server.name} ${enabled ? "enabled" : "cut"} for ${currentAgent.displayName} · ${scopeSuffix()}`,
      },
    );
  }

  function toggleLive(row: LiveRow, enabled: boolean) {
    if (row.kind === "plugin") {
      const plugin = currentTab.dto?.plugins.find((item) => item.id === row.id);
      if (plugin) {
        togglePlugin(plugin, enabled);
      }
      return;
    }
    if (row.kind === "mcp") {
      const server = currentTab.dto?.mcpServers.find((item) => item.id === row.id);
      if (server?.togglable) {
        toggleMcp(server, enabled);
      }
      return;
    }
    const skill =
      currentTab.dto?.userSkills.find((item) => item.id === row.id) ??
      currentTab.dto?.plugins.flatMap((plugin) => plugin.skills).find((item) => item.id === row.id);
    if (skill?.togglable) {
      toggleSkill(skill, enabled);
    }
  }

  async function masterCut(enabled: boolean) {
    if (!showMasterCut) {
      return;
    }
    const agentId = selected;
    const dto = tabs[agentId].dto;
    if (!dto) {
      return;
    }
    await withAgentLock(tabs, agentId, async () => {
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
        tabs[agentId].error = null;
        note(
          enabled ? "ON" : "TRIP",
          `master ${enabled ? "restored" : "cut"} for ${agentLabel(agentId)} · ${scopeSuffix(agentId)}`,
        );
      } catch (error) {
        tabs[agentId].dto = current;
        tabs[agentId].error = displayError(parseInvokeError(error), agentLabel(agentId));
        note("TRIP", `master cut failed for ${agentLabel(agentId)}`);
      }
    });
  }

  async function pickFolder(): Promise<string | null> {
    const dir = await open({ directory: true, multiple: false });
    return typeof dir === "string" ? dir : null;
  }

  async function install(source: string) {
    installError = null;
    const agentId = selected;
    await withAgentLock(tabs, agentId, async () => {
      try {
        const next = await api.installPlugin(agentId, source);
        await applyScopedTab(agentId, next);
        tabs[agentId].error = null;
        installOpen = false;
        screen = "plugins";
        note("INST", `${source} installed · enabled on ${agentLabel(agentId)}`);
      } catch (error) {
        installError = displayError(parseInvokeError(error), agentLabel(agentId));
        note("TRIP", installError);
      }
    });
  }

  function updatePlugin(plugin: PluginDto) {
    void mutate(
      () => api.updatePlugin(selected, plugin.id),
      { tag: "SYNC", text: `${plugin.name} updated on ${currentAgent.displayName}` },
    );
  }

  async function confirmUninstall() {
    if (!uninstallTarget) {
      return;
    }
    const target = uninstallTarget;
    await mutate(
      () => api.uninstallPlugin(selected, target.id),
      { tag: "TRIP", text: `${target.name} uninstalled from ${currentAgent.displayName}` },
    );
    uninstallTarget = null;
  }

  function onFilterKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      tabs[selected].filter = "";
    }
  }

  function onAgentTabKey(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
      return;
    }
    event.preventDefault();
    const index = agents.findIndex((agent) => agent.id === selected);
    if (index < 0) {
      return;
    }
    const next =
      event.key === "ArrowRight"
        ? agents[(index + 1) % agents.length]
        : agents[(index - 1 + agents.length) % agents.length];
    selected = next.id;
  }
</script>

<div class="flex h-full min-h-full flex-col bg-[var(--void)] text-[var(--silkscreen)]">
  <div class="flex h-[38px] shrink-0 items-center gap-3 border-b border-[var(--hair)] bg-[var(--plate)] px-3">
    <div class="flex-1 text-center text-[11.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
      {usageScreen ? "on-n-off — usage" : `on-n-off — ${currentAgent.displayName} panel`}
    </div>
    <Rocker
      size="theme"
      on={theme === "light"}
      offLabel="DARK"
      onLabel="LIGHT"
      ariaLabel="Theme"
      onToggle={() => (theme = theme === "dark" ? "light" : "dark")}
    />
  </div>

  <header class="flex items-center gap-3.5 border-b border-[var(--hair)] bg-[var(--plate)] px-4 py-[11px]">
    <div class="shrink-0 text-[15px] font-bold tracking-[0.09em]">ON-N-OFF</div>
    {#if !usageScreen}
      <div
        class="inline-grid grid-flow-col overflow-hidden rounded-lg border border-[var(--hair)]"
        role="tablist"
        aria-label="Agent"
        tabindex="-1"
        onkeydown={onAgentTabKey}
      >
        {#each agents as agent (agent.id)}
          <button
            type="button"
            role="tab"
            id={`agent-tab-${agent.id}`}
            aria-selected={selected === agent.id}
            aria-controls="agent-panel"
            tabindex={selected === agent.id ? 0 : -1}
            class="h-[30px] min-w-[92px] cursor-pointer rounded-none border-0 px-3 text-[11.5px] font-semibold tracking-[0.05em] uppercase {selected ===
            agent.id
              ? 'bg-[var(--fill)] text-[var(--fill-ink)]'
              : 'bg-[var(--plate)] text-[var(--mute)]'}"
            onclick={() => (selected = agent.id)}
          >
            {agent.displayName}
          </button>
        {/each}
      </div>
      <div class="flex items-center gap-2 pl-1" aria-label={currentAgent.cliOk ? `${currentAgent.displayName} online` : `${currentAgent.displayName} offline`}>
        <span
          class="size-[7px] rounded-full {currentAgent.cliOk
            ? 'bg-[var(--live)] shadow-[0_0_8px_var(--live)]'
            : 'bg-[var(--mute)]'}"
          aria-hidden="true"
        ></span>
        <span class="font-mono text-[11.5px] text-[var(--mute)]">{cliLine}</span>
      </div>
    {:else}
      <div class="font-mono text-[11.5px] text-[var(--mute)]">usage · local transcripts</div>
    {/if}
    <div class="flex-1"></div>
    {#if !usageScreen}
      <label class="flex h-8 items-center gap-2 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-2.5">
        <span class="text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)]">FILTER</span>
        <span class="sr-only">{copy.filterPlaceholder}</span>
        <input
          class="w-[180px] bg-transparent text-[13px] placeholder:text-[var(--mute)] focus-visible:outline-none"
          type="search"
          placeholder={copy.filterPlaceholder}
          bind:value={tabs[selected].filter}
          onkeydown={onFilterKey}
        />
      </label>
      <button
        type="button"
        class="flex size-8 items-center justify-center rounded-lg border border-[var(--hair)] bg-[var(--well)] text-[var(--silkscreen)]"
        title={copy.refresh}
        aria-label={copy.refresh}
        onclick={() => void loadTab(selected, true)}
      >
        <RefreshCw class="size-3.5" aria-hidden="true" />
      </button>
      <button
        type="button"
        class="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-3.5 text-[11.5px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] disabled:opacity-45"
        disabled={!canInstall}
        onclick={() => (installOpen = true)}
      >
        + INSTALL
      </button>
    {/if}
  </header>

  {#if !usageScreen}
    <ScopeBar
      agentId={selected}
      projects={currentProjects}
      selectedPath={currentScopePath}
      note={scopeNote}
      tally={tallyLine(counts, currentAgent.displayName)}
      globalItems={globalItemCount(currentTab.dto)}
      onSelect={(path) => void selectScope(path)}
      onPickFolder={() => void pickProjectFolder()}
      onOpenPath={(path) => void openProjectPath(path)}
    />
  {/if}

  <div class="flex min-h-0 flex-1">
    <LeftRail
      {screen}
      {counts}
      masterOn={allOn}
      {masterNote}
      {showMasterCut}
      busy={currentTab.inFlight}
      masterDisabled={!currentTab.dto || currentTab.inFlight}
      onScreen={(next) => (screen = next)}
      onMaster={(enabled) => void masterCut(enabled)}
    />
    <div id="agent-panel" class="min-w-0 flex-1 overflow-y-auto bg-[var(--void)]" role="tabpanel" aria-labelledby={usageScreen ? undefined : `agent-tab-${selected}`}>
      {#if usageVisited}
        <div class={usageScreen ? "contents" : "hidden"} aria-hidden={!usageScreen}>
          <Usage />
        </div>
      {/if}
      {#if !usageScreen && banner}
        <AgentBanner message={banner} />
      {/if}
      {#if !usageScreen && currentTab.loading && !currentTab.dto}
        <p class="m-5 text-[13px] text-[var(--mute)]">Loading {currentAgent.displayName}…</p>
      {:else if screen === "overview"}
        <Overview
          {counts}
          rows={live}
          {drift}
          {log}
          busy={currentTab.inFlight}
          pluginToggle={currentAgent.pluginToggle}
          scopeLabel={currentScopeLabel}
          onToggle={toggleLive}
          onUpdate={(pluginId) => {
            const plugin = currentTab.dto?.plugins.find((item) => item.id === pluginId);
            if (plugin) {
              updatePlugin(plugin);
            }
          }}
          cliOk={currentAgent.cliOk}
        />
      {:else if screen === "plugins"}
        <ItemList
          kind="plugin"
          tab={{ plugins: filtered?.plugins ?? [], userSkills: [], mcpServers: filtered?.mcpServers ?? [] }}
          filterQuery={currentTab.filter}
          {expandedIds}
          cliOk={currentAgent.cliOk}
          pluginToggle={currentAgent.pluginToggle}
          busy={currentTab.inFlight}
          onToggleExpand={toggleExpand}
          onTogglePlugin={togglePlugin}
          onToggleSkill={toggleSkill}
          onUninstall={(plugin) => (uninstallTarget = plugin)}
          onUpdate={updatePlugin}
        />
      {:else if screen === "skills"}
        <ItemList
          kind="skill"
          tab={currentTab.dto ?? emptyTabDto()}
          filterQuery={currentTab.filter}
          {expandedIds}
          cliOk={currentAgent.cliOk}
          pluginToggle={currentAgent.pluginToggle}
          busy={currentTab.inFlight}
          onToggleExpand={toggleExpand}
          onTogglePlugin={togglePlugin}
          onToggleSkill={toggleSkill}
          onUninstall={(plugin) => (uninstallTarget = plugin)}
        />
      {:else if screen === "mcp"}
        <McpList
          tab={currentTab.dto ?? emptyTabDto()}
          filterQuery={currentTab.filter}
          busy={currentTab.inFlight}
          onToggle={toggleMcp}
        />
      {:else if screen === "config"}
        <AgentConfig agent={currentAgent} tab={currentTab.dto} projects={currentProjects} selectedPath={currentScopePath} />
      {/if}
    </div>
  </div>
</div>

{#if installOpen}
  <InstallSheet
    agentName={currentAgent.displayName}
    busy={currentTab.inFlight}
    error={installError}
    installFolder={currentAgent.installFolder}
    onCancel={() => {
      installOpen = false;
      installError = null;
    }}
    onInstall={(source) => void install(source)}
    onPickFolder={pickFolder}
  />
{/if}

{#if uninstallTarget}
  <ConfirmDialog
    title={copy.uninstallTitle(uninstallTarget.name)}
    body={copy.uninstallBody(currentAgent.displayName)}
    busy={currentTab.inFlight}
    onCancel={() => (uninstallTarget = null)}
    onConfirm={() => void confirmUninstall()}
  />
{/if}

<style>
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
