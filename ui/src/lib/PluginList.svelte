<script lang="ts">
  import { copy } from "./copy";
  import Rocker from "./Rocker.svelte";
  import SkillRow from "./SkillRow.svelte";
  import type { AgentTabDto, PluginDto, SkillDto } from "./types";

  let {
    tab,
    filterQuery = "",
    emptyBecauseFilter = false,
    filtering = false,
    expandedIds,
    cliOk,
    pluginToggle,
    busy = false,
    onToggleExpand,
    onTogglePlugin,
    onToggleSkill,
    onUninstall,
  }: {
    tab: AgentTabDto;
    filterQuery?: string;
    emptyBecauseFilter?: boolean;
    filtering?: boolean;
    expandedIds: Set<string>;
    cliOk: boolean;
    pluginToggle: boolean;
    busy?: boolean;
    onToggleExpand: (pluginId: string) => void;
    onTogglePlugin: (plugin: PluginDto, enabled: boolean) => void;
    onToggleSkill: (skill: SkillDto, enabled: boolean) => void;
    onUninstall: (plugin: PluginDto) => void;
  } = $props();

  const pluginCounts = $derived({
    on: tab.plugins.filter((plugin) => plugin.enabled).length,
    off: tab.plugins.filter((plugin) => !plugin.enabled).length,
  });
  const skillCounts = $derived({
    on: tab.userSkills.filter((skill) => skill.enabled).length,
    off: tab.userSkills.filter((skill) => !skill.enabled).length,
  });
</script>

{#if emptyBecauseFilter}
  <p class="empty">{copy.filterMiss(filterQuery)}</p>
{:else}
  <section class="section">
    <header class="bar">
      <h2>Plugins</h2>
      <span class="counts">{pluginCounts.on} on · {pluginCounts.off} off</span>
    </header>
    {#if tab.plugins.length === 0}
      {#if !filtering}
        <p class="empty">{copy.emptyPlugins}</p>
      {/if}
    {:else}
      <div class="list">
        {#each tab.plugins as plugin (plugin.id)}
          {@const open = expandedIds.has(plugin.id)}
          <article class="plugin">
            <div class="plugin-row">
              <button
                type="button"
                class="chevron"
                aria-expanded={open}
                aria-label={open ? `Collapse ${plugin.name}` : `Expand ${plugin.name}`}
                onclick={() => onToggleExpand(plugin.id)}
              >
                {open ? "▾" : "▸"}
              </button>
              <div class="meta">
                <div class="name">{plugin.name}</div>
                <div class="id">{plugin.id}</div>
              </div>
              <span class="source">{plugin.source}</span>
              <Rocker
                size="plugin"
                on={plugin.enabled}
                busy={busy}
                disabled={!pluginToggle}
                ariaLabel={`${plugin.name} ${plugin.enabled ? "on" : "off"}`}
                onToggle={() => onTogglePlugin(plugin, !plugin.enabled)}
              />
            </div>
            {#if open}
              <div class="well">
                <div class="id-row">
                  <span class="id">{plugin.id}</span>
                  <button
                    type="button"
                    class="more"
                    aria-label={`Uninstall ${plugin.name}`}
                    disabled={!cliOk || busy}
                    onclick={() => onUninstall(plugin)}
                  >
                    ⋯
                  </button>
                </div>
                {#each plugin.skills as skill (skill.id)}
                  <SkillRow {skill} {busy} onToggle={(enabled) => onToggleSkill(skill, enabled)} />
                {/each}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {/if}
  </section>

  <section class="section">
    <header class="bar">
      <h2>User skills</h2>
      <span class="counts">{skillCounts.on} on · {skillCounts.off} off</span>
    </header>
    {#if tab.userSkills.length === 0}
      {#if !filtering}
        <p class="empty">{copy.emptyUserSkills}</p>
      {/if}
    {:else}
      <div class="list">
        {#each tab.userSkills as skill (skill.id)}
          <div class="user-row">
            <SkillRow
              {skill}
              source="user"
              {busy}
              onToggle={(enabled) => onToggleSkill(skill, enabled)}
            />
          </div>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  .section {
    margin: 0 16px 18px;
  }

  .bar {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    background: var(--plate);
    padding: 6px 10px;
    margin-bottom: 8px;
  }

  h2 {
    margin: 0;
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-size: 13px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    font-weight: 600;
  }

  .counts,
  .source,
  .id,
  .empty {
    color: var(--mute);
    font-size: 12px;
  }

  .id {
    font-family: "IBM Plex Mono", monospace;
  }

  .empty {
    margin: 12px 10px;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .plugin,
  .user-row {
    background: var(--plate);
  }

  .plugin-row,
  .id-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
  }

  .chevron {
    border: 0;
    background: transparent;
    color: var(--silkscreen);
    cursor: pointer;
    width: 24px;
    padding: 0;
  }

  .meta {
    flex: 1;
    min-width: 0;
  }

  .name {
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-weight: 600;
    font-size: 16px;
  }

  .well {
    background: var(--well);
    margin: 0 8px 8px 34px;
  }

  .more {
    margin-left: auto;
    border: 0;
    background: transparent;
    color: var(--mute);
    cursor: pointer;
    font-size: 18px;
    line-height: 1;
  }

  .more:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }
</style>
