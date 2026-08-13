<script lang="ts">
  import ChevronRight from "lucide-svelte/icons/chevron-right";
  import { copy } from "./copy";
  import Rocker from "./Rocker.svelte";
  import SkillRow from "./SkillRow.svelte";
  import { formatPluginVersion, canUninstallPlugin, pluginOutOfSync, pluginVersionNote, skillIsLive, sortPlugins } from "./catalog";
  import { isProjectOrigin } from "./project";
  import { filterSkillList } from "./filterTab";
  import type { AgentTabDto, PluginDto, SkillDto } from "./types";

  type Chip = "all" | "on" | "off" | "behind";
  type Kind = "plugin" | "skill";

  let {
    kind,
    tab,
    filterQuery = "",
    expandedIds,
    cliOk,
    pluginToggle,
    busy = false,
    onToggleExpand,
    onTogglePlugin,
    onToggleSkill,
    onUninstall,
    onUpdate,
  }: {
    kind: Kind;
    tab: AgentTabDto;
    filterQuery?: string;
    expandedIds: Set<string>;
    cliOk: boolean;
    pluginToggle: boolean;
    busy?: boolean;
    onToggleExpand: (pluginId: string) => void;
    onTogglePlugin: (plugin: PluginDto, enabled: boolean) => void;
    onToggleSkill: (skill: SkillDto, enabled: boolean) => void;
    onUninstall: (plugin: PluginDto) => void;
    onUpdate?: (plugin: PluginDto) => void;
  } = $props();

  let chip = $state<Chip>("all");

  const plugins = $derived(
    chip === "behind"
      ? sortPlugins(tab.plugins).filter((plugin) => pluginOutOfSync(plugin))
      : applyChip(sortPlugins(tab.plugins), chip, (plugin) => plugin.enabled),
  );
  const skillPool = $derived(filterSkillList(tab, filterQuery));
  const skills = $derived(
    applyChip(
      skillPool,
      chip,
      (skill) => skillIsLive(skill, tab),
    ),
  );

  const title = $derived(kind === "plugin" ? "Installed plugins" : "Skills library");
  const subtitle = $derived(
    kind === "plugin"
      ? `${tab.plugins.filter((plugin) => plugin.enabled).length} live · installed from git, marketplace or folder`
      : "plugin skills follow their plugin · user skills toggle · project skills stay with the repo",
  );
  const chips = $derived(kind === "plugin" ? (["all", "on", "off", "behind"] as Chip[]) : (["all", "on", "off"] as Chip[]));

  function applyChip<T>(items: T[], next: Chip, enabled: (item: T) => boolean): T[] {
    if (next === "on") {
      return items.filter((item) => enabled(item));
    }
    if (next === "off") {
      return items.filter((item) => !enabled(item));
    }
    return items;
  }
</script>

<div class="flex flex-col gap-3.5 px-5 pt-[18px] pb-[26px]">
  <header class="flex items-baseline gap-3">
    <h2 class="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">{title}</h2>
    <span class="font-mono text-[12px]/[1.3] text-[var(--mute)]">{subtitle}</span>
    <div class="flex-1"></div>
    <div class="flex border border-[var(--hair)]" role="group" aria-label="Filter list">
      {#each chips as next (next)}
        <button
          type="button"
          class="h-[26px] rounded-none border-0 px-3 text-[10.5px] font-semibold tracking-[0.03em] uppercase {chip === next
            ? 'bg-[var(--well)] text-[var(--silkscreen)]'
            : 'bg-transparent text-[var(--mute)]'}"
          aria-pressed={chip === next}
          onclick={() => (chip = next)}
        >
          {next}
        </button>
      {/each}
    </div>
  </header>

  {#if kind === "plugin"}
    {#if tab.plugins.length === 0}
      <p class="text-[13px] text-[var(--mute)]">{filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyPlugins}</p>
    {:else if plugins.length === 0}
      <p class="text-[13px] text-[var(--mute)]">
        {chip === "behind" ? "No plugins behind." : chip === "on" ? "No plugins on." : "No plugins off."}
      </p>
    {:else}
      <div class="flex flex-col gap-1.5">
        {#each plugins as plugin (plugin.id)}
          {@const open = expandedIds.has(plugin.id)}
          <article class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
            <div class="flex items-center gap-3 px-3 py-[11px]">
              <button
                type="button"
                class="flex min-h-6 min-w-6 shrink-0 items-center justify-center border-0 bg-transparent p-0 text-[var(--silkscreen)]"
                aria-expanded={open}
                aria-label={open ? `Collapse ${plugin.name}` : `Expand ${plugin.name}`}
                onclick={() => onToggleExpand(plugin.id)}
              >
                <ChevronRight class="size-4 transition-transform duration-150 {open ? 'rotate-90' : ''}" aria-hidden="true" />
              </button>
              <span
                class="size-2 shrink-0 rounded-full {plugin.enabled
                  ? 'bg-[var(--live)] shadow-[0_0_7px_var(--live)]'
                  : 'bg-[var(--mute)]'}"
                aria-hidden="true"
              ></span>
              <div class="w-[238px] min-w-0 shrink-0">
                <div class="text-[16px]/[1.15] font-semibold break-words">{plugin.name}</div>
                <div class="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]" title={plugin.source}>{plugin.source}</div>
              </div>
              <div class="w-[112px] shrink-0">
                {#if plugin.version}
                  <div class="font-mono text-[12px]/[1.3] font-medium text-[var(--silkscreen)]">
                    {formatPluginVersion(plugin.version)}
                  </div>
                  <div
                    class="font-mono text-[10.5px]/[1.3] {pluginOutOfSync(plugin)
                      ? 'text-[var(--warn)]'
                      : 'text-[var(--mute)]'}"
                  >
                    {pluginVersionNote(plugin)}
                  </div>
                {:else}
                  <div class="font-mono text-[12px]/[1.3] text-[var(--mute)]">—</div>
                {/if}
              </div>
              {#if pluginOutOfSync(plugin)}
                <button
                  type="button"
                  class="h-[26px] shrink-0 border border-[var(--warn)] bg-transparent px-3 text-[10.5px] font-semibold tracking-[0.03em] text-[var(--warn)] uppercase disabled:opacity-45"
                  disabled={!cliOk || busy}
                  aria-label={`Update ${plugin.name}`}
                  onclick={() => onUpdate?.(plugin)}
                >
                  {copy.update}
                </button>
              {/if}
              <div class="ml-auto shrink-0">
                <Rocker
                  size="plugin"
                  on={plugin.enabled}
                  {busy}
                  disabled={!pluginToggle || !plugin.togglable}
                  ariaLabel={`${plugin.name} ${plugin.enabled ? "on" : "off"}`}
                  onToggle={() => onTogglePlugin(plugin, !plugin.enabled)}
                />
              </div>
            </div>
            {#if open}
              <div class="mx-3 mb-3 ml-[46px] border border-[var(--hair)] bg-[var(--well)]">
                <div class="flex items-center gap-2.5 border-b border-[var(--hair)] px-[11px] py-2">
                  <span class="min-w-0 flex-1 truncate font-mono text-[11.5px] text-[var(--mute)]" title={plugin.id}>{plugin.id}</span>
                  <button
                    type="button"
                    class="h-6 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] px-2.5 text-[10px] font-semibold tracking-[0.03em] text-[var(--trip)] uppercase disabled:opacity-45"
                    aria-label={`Uninstall ${plugin.name}`}
                    disabled={!cliOk || busy || !canUninstallPlugin(plugin)}
                    onclick={() => onUninstall(plugin)}
                  >
                    Uninstall
                  </button>
                </div>
                {#each plugin.skills as skill (skill.id)}
                  <SkillRow
                    {skill}
                    live={skill.togglable ? skill.enabled : plugin.enabled}
                    {busy}
                    onToggle={(enabled) => onToggleSkill(skill, enabled)}
                  />
                {/each}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {/if}
  {:else if skillPool.length === 0}
    <p class="text-[13px] text-[var(--mute)]">{filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyUserSkills}</p>
  {:else if skills.length === 0}
    <p class="text-[13px] text-[var(--mute)]">{chip === "on" ? "No skills on." : "No skills off."}</p>
  {:else}
    <div class="flex flex-col gap-1.5">
      {#each skills as skill (skill.id)}
        {@const live = skill.togglable
          ? skill.enabled
          : tab.plugins.find((plugin) => plugin.id === skill.pluginId)?.enabled ?? false}
        {@const lockedNote = isProjectOrigin(skill.origin) ? copy.skillProject : copy.skillLocked}
        <article class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
          <div class="flex items-center gap-3 px-3 py-[11px]">
            <span class="flex min-h-6 min-w-6 shrink-0 items-center justify-center text-[var(--mute)]" aria-hidden="true">·</span>
            <span
              class="size-2 shrink-0 rounded-full {live ? 'bg-[var(--live)] shadow-[0_0_7px_var(--live)]' : 'bg-[var(--mute)]'}"
              aria-hidden="true"
            ></span>
            <div class="w-[238px] min-w-0 shrink-0">
              <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                <span class="text-[16px]/[1.15] font-semibold break-words">{skill.name}</span>
                <span class="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--mute)]"
                  >{isProjectOrigin(skill.origin) ? "Project skill" : skill.pluginId ? "Plugin skill" : "User skill"}</span
                >
              </div>
              <div class="mt-0.5 text-[11.5px] leading-snug break-words text-[var(--mute)]">{skill.description || skill.id}</div>
            </div>
            {#if skill.togglable}
              <div class="ml-auto shrink-0">
                <Rocker
                  size="plugin"
                  on={skill.enabled}
                  {busy}
                  ariaLabel={`${skill.name} ${skill.enabled ? "on" : "off"}`}
                  onToggle={() => onToggleSkill(skill, !skill.enabled)}
                />
              </div>
            {:else}
              <span
                class="ml-auto flex min-w-[88px] shrink-0 items-center gap-[7px] font-mono text-[10.5px] text-[var(--mute)]"
                title={lockedNote}
              >
                <span class="size-2 shrink-0 rounded-full bg-[var(--mute)]" aria-hidden="true"></span>
                {isProjectOrigin(skill.origin) ? "project" : "with plugin"}
                <span class="sr-only">{lockedNote}</span>
              </span>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>
