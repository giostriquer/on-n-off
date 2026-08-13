<script lang="ts">
  import { onMount } from "svelte";
  import AgentBanner from "$lib/AgentBanner.svelte";
  import ConfirmDialog from "$lib/ConfirmDialog.svelte";
  import InstallSheet from "$lib/InstallSheet.svelte";
  import PluginList from "$lib/PluginList.svelte";
  import Rocker from "$lib/Rocker.svelte";
  import * as api from "$lib/api";
  import { copy } from "$lib/copy";
  import { displayError, parseInvokeError } from "$lib/error";
  import { filterTab } from "$lib/filterTab";
  import { LOCKED_AGENTS, emptyTab, openIds, overlayAgents, withAgentLock, type TabState } from "$lib/session";
  import type { AgentId, AgentInfo, AgentTabDto, PluginDto, SkillDto } from "$lib/types";

  const THEME_KEY = "on-n-off.theme";
  const AGENT_KEY = "on-n-off.agent";
  type Theme = "dark" | "light";

  let theme = $state<Theme>(readTheme());
  let agents = $state<AgentInfo[]>(LOCKED_AGENTS.map((agent) => ({ ...agent })));
  let selected = $state<AgentId>(readAgent());
  let tabs = $state<Record<AgentId, TabState>>({
    claude: emptyTab(),
    codex: emptyTab(),
  });
  let installOpen = $state(false);
  let installError = $state<string | null>(null);
  let uninstallTarget = $state<PluginDto | null>(null);

  const currentAgent = $derived(agents.find((agent) => agent.id === selected) ?? agents[0]);
  const currentTab = $derived(tabs[selected]);
  const filtered = $derived(currentTab.dto ? filterTab(currentTab.dto, currentTab.filter) : null);
  const expandedIds = $derived(openIds(currentTab.expanded, filtered?.expandIds ?? []));
  const banner = $derived(bannerMessage(currentAgent, currentTab.error));
  const canInstall = $derived(currentAgent.cliOk && currentAgent.installGit && !currentTab.inFlight);

  $effect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  });

  $effect(() => {
    localStorage.setItem(AGENT_KEY, selected);
  });

  onMount(() => {
    void boot();
  });

  function readTheme(): Theme {
    return localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
  }

  function readAgent(): AgentId {
    return localStorage.getItem(AGENT_KEY) === "codex" ? "codex" : "claude";
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
      agents = overlayAgents(await api.listAgents());
    } catch (error) {
      agents = overlayAgents([]);
      tabs[selected].error = displayError(parseInvokeError(error), currentAgent.displayName);
    }
    await Promise.all([loadTab("claude"), loadTab("codex")]);
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
        tabs[agentId].dto = await api.refresh(agentId);
        tabs[agentId].error = null;
      } catch (error) {
        tabs[agentId].dto = null;
        tabs[agentId].error = displayError(parseInvokeError(error), agentLabel(agentId));
      } finally {
        tabs[agentId].loading = false;
      }
    });
  }

  function agentLabel(agentId: AgentId): string {
    return agents.find((agent) => agent.id === agentId)?.displayName ?? agentId;
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

  async function mutate(fn: () => Promise<AgentTabDto>) {
    const agentId = selected;
    await withAgentLock(tabs, agentId, async () => {
      try {
        tabs[agentId].dto = await fn();
        tabs[agentId].error = null;
      } catch (error) {
        tabs[agentId].error = displayError(parseInvokeError(error), agentLabel(agentId));
      }
    });
  }

  function togglePlugin(plugin: PluginDto, enabled: boolean) {
    void mutate(() => api.setPluginEnabled(selected, plugin.id, enabled));
  }

  function toggleSkill(skill: SkillDto, enabled: boolean) {
    void mutate(() => api.setSkillEnabled(selected, skill.id, enabled));
  }

  async function install(source: string) {
    installError = null;
    const agentId = selected;
    await withAgentLock(tabs, agentId, async () => {
      try {
        tabs[agentId].dto = await api.installPlugin(agentId, source);
        tabs[agentId].error = null;
        installOpen = false;
      } catch (error) {
        installError = displayError(parseInvokeError(error), agentLabel(agentId));
      }
    });
  }

  async function confirmUninstall() {
    if (!uninstallTarget) {
      return;
    }
    const target = uninstallTarget;
    await mutate(() => api.uninstallPlugin(selected, target.id));
    uninstallTarget = null;
  }

  function onFilterKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      tabs[selected].filter = "";
    }
  }
</script>

<div class="app">
  <header class="plate">
    <div class="mark">ON-N-OFF</div>
    <div class="tabs" role="tablist" aria-label="Agent">
      {#each agents as agent (agent.id)}
        <button
          type="button"
          role="tab"
          aria-selected={selected === agent.id}
          class:on={selected === agent.id}
          onclick={() => (selected = agent.id)}
        >
          {agent.displayName}
        </button>
      {/each}
    </div>
    <div class="spacer"></div>
    <Rocker
      size="tab"
      on={theme === "light"}
      offLabel="Dark"
      onLabel="Light"
      ariaLabel="Theme"
      onToggle={() => (theme = theme === "dark" ? "light" : "dark")}
    />
    <button type="button" class="icon" title={copy.refresh} onclick={() => void loadTab(selected, true)}>
      ↻
    </button>
    <button type="button" class="install" disabled={!canInstall} onclick={() => (installOpen = true)}>
      + {copy.install}
    </button>
  </header>

  {#if banner}
    <AgentBanner message={banner} />
  {/if}

  <label class="filter">
    <span class="sr-only">{copy.filterPlaceholder}</span>
    <input
      type="search"
      placeholder={copy.filterPlaceholder}
      bind:value={tabs[selected].filter}
      onkeydown={onFilterKey}
    />
  </label>

  {#if currentTab.loading && !currentTab.dto}
    <p class="status">Loading {currentAgent.displayName}…</p>
  {:else if filtered}
    <PluginList
      tab={{ plugins: filtered.plugins, userSkills: filtered.userSkills }}
      filterQuery={currentTab.filter}
      emptyBecauseFilter={filtered.emptyBecauseFilter}
      filtering={Boolean(currentTab.filter.trim())}
      {expandedIds}
      cliOk={currentAgent.cliOk}
      pluginToggle={currentAgent.pluginToggle}
      busy={currentTab.inFlight}
      onToggleExpand={toggleExpand}
      onTogglePlugin={togglePlugin}
      onToggleSkill={toggleSkill}
      onUninstall={(plugin) => (uninstallTarget = plugin)}
    />
  {/if}
</div>

{#if installOpen}
  <InstallSheet
    agentName={currentAgent.displayName}
    busy={currentTab.inFlight}
    error={installError}
    onCancel={() => {
      installOpen = false;
      installError = null;
    }}
    onInstall={(source) => void install(source)}
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
  .app {
    min-height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--void);
  }

  .plate {
    display: flex;
    align-items: center;
    gap: 12px;
    background: var(--plate);
    padding: 10px 16px;
  }

  .mark {
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-weight: 700;
    letter-spacing: 0.18em;
    font-size: 15px;
  }

  .tabs {
    display: inline-grid;
    grid-template-columns: 1fr 1fr;
    border: 1px solid var(--mute);
  }

  .tabs button {
    height: 36px;
    min-width: 88px;
    border: 0;
    background: var(--plate);
    color: var(--mute);
    cursor: pointer;
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-weight: 600;
    letter-spacing: 0.06em;
  }

  .tabs button.on {
    background: var(--brass);
    color: var(--void);
  }

  .spacer {
    flex: 1;
  }

  .icon,
  .install {
    height: 36px;
    border: 1px solid var(--mute);
    background: var(--well);
    color: var(--silkscreen);
    cursor: pointer;
    padding: 0 12px;
  }

  .install:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .filter {
    display: block;
    margin: 12px 16px;
  }

  .filter input {
    width: 100%;
    box-sizing: border-box;
    background: var(--well);
    color: var(--silkscreen);
    border: 1px solid var(--mute);
    padding: 8px 10px;
  }

  .status {
    margin: 16px;
    color: var(--mute);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
