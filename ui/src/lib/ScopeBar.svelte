<script lang="ts">
  import { onMount, tick } from "svelte";
  import { scopeConfigPath } from "./catalog";
  import { looksLikeFolderPath, projectLabel, sameProjectPath, scopeChip } from "./project";
  import type { AgentId, ProjectDto } from "./types";

  type Props = {
    agentId: AgentId;
    projects: ProjectDto[];
    selectedPath: string | null;
    note: string;
    tally: string;
    globalItems: number;
    onSelect: (path: string | null) => void;
    onPickFolder: () => void;
    onOpenPath: (path: string) => void;
  };

  let {
    agentId,
    projects,
    selectedPath,
    note,
    tally,
    globalItems,
    onSelect,
    onPickFolder,
    onOpenPath,
  }: Props = $props();

  let open = $state(false);
  let query = $state("");
  let searchEl = $state<HTMLInputElement | undefined>(undefined);

  const selected = $derived(
    selectedPath ? (projects.find((project) => sameProjectPath(project.path, selectedPath)) ?? null) : null,
  );
  const title = $derived(selected?.label ?? "All projects");
  const pathLine = $derived(selected?.path ?? scopeConfigPath(agentId));
  const chip = $derived(scopeChip(selected));
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      return projects;
    }
    return projects.filter(
      (project) => project.label.toLowerCase().includes(q) || project.path.toLowerCase().includes(q),
    );
  });
  const pastePath = $derived(looksLikeFolderPath(query) ? query.trim() : "");
  const queryKey = $derived(query.trim().toLowerCase());
  const showAllRow = $derived(!queryKey || "all projects global".includes(queryKey));
  const showEmpty = $derived(!!queryKey && !showAllRow && filtered.length === 0 && !pastePath);

  onMount(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closePicker();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  function closePicker() {
    open = false;
    query = "";
  }

  async function togglePicker() {
    open = !open;
    if (!open) {
      query = "";
      return;
    }
    await tick();
    searchEl?.focus();
  }

  function chooseAll() {
    onSelect(null);
    closePicker();
  }

  function chooseProject(path: string) {
    onSelect(path);
    closePicker();
  }

  function choosePaste() {
    if (!pastePath) {
      return;
    }
    onOpenPath(pastePath);
    closePicker();
  }

  function chooseFolder() {
    closePicker();
    onPickFolder();
  }
</script>

{#snippet folderIcon(on: boolean)}
  <span class="scope-folder" class:is-on={on} aria-hidden="true"></span>
{/snippet}

<div class="scope-bar">
  <span class="scope-label">SCOPE</span>

  <button type="button" class="scope-trigger" class:is-open={open} aria-expanded={open} aria-haspopup="dialog" onclick={() => void togglePicker()}>
    {@render folderIcon(true)}
    <span class="scope-trigger-copy">
      <span class="scope-trigger-title">{title}</span>
      <span class="scope-trigger-path" title={pathLine}>{pathLine}</span>
    </span>
    <span class="scope-chevron">▾</span>
  </button>

  <span class="scope-chip" class:is-project={!!selected}>{chip}</span>
  <span class="scope-note" title={note}>{note}</span>
  <span class="scope-spacer"></span>
  <span class="scope-tally">{tally}</span>

  {#if open}
    <button type="button" class="scope-backdrop" tabindex="-1" aria-label="Close project picker" onclick={closePicker}></button>
    <div class="scope-picker" role="dialog" aria-modal="true" aria-label="Choose scope">
      <div class="scope-search">
        <span class="scope-search-icon" aria-hidden="true">⌕</span>
        <input
          bind:this={searchEl}
          bind:value={query}
          class="scope-search-input"
          placeholder="Search projects, or paste a folder path…"
          aria-label="Search projects, or paste a folder path"
        />
        <span class="scope-esc">esc</span>
      </div>

      <div class="scope-list">
        {#if showAllRow}
          <button type="button" class="scope-row" class:is-active={!selected} onclick={chooseAll}>
            {@render folderIcon(!selected)}
            <span class="scope-row-copy">
              <span class="scope-row-head">
                <span class="scope-row-name">All projects</span>
                <span class="scope-badge">GLOBAL</span>
              </span>
              <span class="scope-row-path" title={scopeConfigPath(agentId)}>{scopeConfigPath(agentId)}</span>
            </span>
            <span class="scope-row-stats">{globalItems} global items</span>
            {#if !selected}
              <span class="scope-check is-on" aria-hidden="true">✓</span>
            {:else}
              <span class="scope-check" aria-hidden="true"></span>
            {/if}
          </button>
        {/if}

        {#each filtered as project (project.id)}
          {@const active = !!selectedPath && sameProjectPath(project.path, selectedPath)}
          <button type="button" class="scope-row" class:is-active={active} onclick={() => chooseProject(project.path)}>
            {@render folderIcon(active)}
            <span class="scope-row-copy">
              <span class="scope-row-head">
                <span class="scope-row-name">{project.label}</span>
                {#if project.branch}
                  <span class="scope-badge">{project.branch}</span>
                {/if}
              </span>
              <span class="scope-row-path" title={project.path}>{project.path}</span>
            </span>
            <span class="scope-row-stats">{`${project.skillCount ?? 0} local skills\n${project.mcpCount ?? 0} project mcps`}</span>
            {#if active}
              <span class="scope-check is-on" aria-hidden="true">✓</span>
            {:else}
              <span class="scope-check" aria-hidden="true"></span>
            {/if}
          </button>
        {/each}

        {#if pastePath}
          <button type="button" class="scope-row" onclick={choosePaste}>
            {@render folderIcon(false)}
            <span class="scope-row-copy">
              <span class="scope-row-head">
                <span class="scope-row-name">Open {projectLabel(pastePath)}</span>
                <span class="scope-badge is-new">NEW</span>
              </span>
              <span class="scope-row-path" title={pastePath}>{pastePath}</span>
            </span>
            <span class="scope-row-stats">scan on open</span>
            <span class="scope-check" aria-hidden="true"></span>
          </button>
        {/if}

        {#if showEmpty}
          <p class="scope-empty">No matching projects.</p>
        {/if}
      </div>

      <div class="scope-foot">
        <button type="button" class="scope-folder-btn" onclick={chooseFolder}>Choose folder…</button>
        <span class="scope-foot-note">Reads .{agentId}/skills and .mcp.json from the folder you pick.</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .scope-bar {
    position: relative;
    z-index: 6;
    display: flex;
    flex: none;
    align-items: center;
    gap: 12px;
    padding: 9px 16px;
    background: var(--void);
    border-bottom: 1px solid var(--hair);
  }
  .scope-label {
    flex: none;
    font: 600 10px/1 "Instrument Sans", sans-serif;
    letter-spacing: 0.05em;
    color: var(--mute);
  }
  .scope-trigger {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 38px;
    max-width: 280px;
    min-width: 0;
    padding: 0 11px;
    border: 1px solid var(--hair);
    border-radius: 9px;
    background: var(--plate);
    color: var(--silkscreen);
    text-align: left;
    cursor: pointer;
  }
  .scope-trigger.is-open {
    border-color: var(--fill);
  }
  .scope-trigger-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    overflow: hidden;
  }
  .scope-trigger-title {
    font: 600 12.5px/1.1 "Instrument Sans", sans-serif;
    white-space: nowrap;
  }
  .scope-trigger-path {
    font: 400 10px/1.1 "JetBrains Mono", monospace;
    color: var(--mute);
    white-space: nowrap;
  }
  .scope-chevron {
    padding-left: 2px;
    font-size: 8px;
    color: var(--mute);
  }
  .scope-chip {
    flex: none;
    padding: 4px 7px;
    border-radius: 5px;
    background: var(--well);
    color: var(--mute);
    font: 500 10px/1 "Instrument Sans", sans-serif;
    letter-spacing: 0.03em;
    white-space: nowrap;
  }
  .scope-chip.is-project {
    color: var(--silkscreen);
  }
  .scope-note {
    min-width: 0;
    overflow: hidden;
    font: 400 11px/1.35 "JetBrains Mono", monospace;
    color: var(--mute);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .scope-spacer {
    flex: 1;
  }
  .scope-tally {
    flex: none;
    font: 400 11.5px/1 "JetBrains Mono", monospace;
    color: var(--mute);
    white-space: nowrap;
  }
  .scope-backdrop {
    position: fixed;
    inset: 0;
    z-index: 28;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    cursor: default;
  }
  .scope-picker {
    position: absolute;
    top: calc(100% + 5px);
    left: 56px;
    z-index: 30;
    width: 452px;
    overflow: hidden;
    border: 1px solid var(--hair);
    border-radius: 12px;
    background: var(--plate);
    box-shadow: var(--drop);
  }
  .scope-search {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 11px 13px;
    border-bottom: 1px solid var(--hair);
  }
  .scope-search-icon {
    flex: none;
    font-size: 13px;
    color: var(--mute);
  }
  .scope-search-input {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--silkscreen);
    font: 400 12.5px/1.3 "JetBrains Mono", monospace;
    outline: none;
  }
  .scope-search-input::placeholder {
    color: var(--mute);
  }
  .scope-esc {
    flex: none;
    padding: 3px 5px;
    border: 1px solid var(--hair);
    border-radius: 4px;
    color: var(--mute);
    font: 500 9.5px/1 "JetBrains Mono", monospace;
  }
  .scope-list {
    max-height: 296px;
    overflow-y: auto;
  }
  .scope-row {
    display: flex;
    width: 100%;
    box-sizing: border-box;
    align-items: center;
    gap: 11px;
    padding: 10px 13px;
    border: 0;
    border-bottom: 1px solid var(--hair);
    border-radius: 0;
    background: transparent;
    color: var(--silkscreen);
    text-align: left;
    cursor: pointer;
  }
  .scope-row:hover,
  .scope-row.is-active {
    background: var(--well);
  }
  .scope-row-copy {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
  }
  .scope-row-head {
    display: flex;
    align-items: baseline;
    gap: 7px;
  }
  .scope-row-name {
    font: 600 13px/1.1 "Instrument Sans", sans-serif;
  }
  .scope-row-path {
    max-width: 250px;
    overflow: hidden;
    font: 400 10.5px/1.2 "JetBrains Mono", monospace;
    color: var(--mute);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .scope-row-stats {
    flex: none;
    font: 400 10.5px/1.5 "JetBrains Mono", monospace;
    color: var(--mute);
    text-align: right;
    white-space: pre-line;
  }
  .scope-check {
    flex: none;
    width: 12px;
    height: 12px;
    font-size: 11px;
    line-height: 12px;
    color: transparent;
  }
  .scope-check.is-on {
    color: var(--brass);
  }
  .scope-badge {
    flex: none;
    padding: 2px 5px;
    border: 1px solid var(--mute);
    border-radius: 4px;
    color: var(--mute);
    font: 600 9px/1 "Instrument Sans", sans-serif;
    letter-spacing: 0.04em;
  }
  .scope-badge.is-new {
    border-color: var(--warn);
    color: var(--warn);
  }
  .scope-empty {
    margin: 0;
    padding: 12px 13px;
    color: var(--mute);
    font: 400 12px/1.3 "Instrument Sans", sans-serif;
  }
  .scope-foot {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 11px 13px;
    border-top: 1px solid var(--hair);
    background: var(--well);
  }
  .scope-folder-btn {
    flex: none;
    height: 30px;
    padding: 0 13px;
    border: 1px solid var(--fill);
    border-radius: 8px;
    background: var(--fill);
    color: var(--fill-ink);
    cursor: pointer;
    font: 600 11px/1 "Instrument Sans", sans-serif;
    letter-spacing: 0.03em;
  }
  .scope-foot-note {
    flex: 1;
    font: 400 10.5px/1.45 "JetBrains Mono", monospace;
    color: var(--mute);
  }
  .scope-folder {
    position: relative;
    flex: none;
    width: 15px;
    height: 12px;
    box-sizing: border-box;
    border: 1.5px solid var(--mute);
    border-radius: 2px;
  }
  .scope-folder::before {
    content: "";
    position: absolute;
    top: -4.5px;
    left: 1.5px;
    width: 6px;
    height: 3px;
    box-sizing: border-box;
    border: 1.5px solid var(--mute);
    border-bottom: 0;
    border-radius: 2px 2px 0 0;
  }
  .scope-folder.is-on,
  .scope-folder.is-on::before {
    border-color: var(--silkscreen);
  }
</style>
