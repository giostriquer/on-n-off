<script lang="ts">
  import { copy } from "./copy";
  import { isProjectOrigin } from "./project";
  import Rocker from "./Rocker.svelte";
  import { filterMcpList } from "./filterTab";
  import type { AgentTabDto, McpServerDto } from "./types";

  type Chip = "all" | "on" | "off";

  let {
    tab,
    filterQuery = "",
    busy = false,
    onToggle,
  }: {
    tab: AgentTabDto;
    filterQuery?: string;
    busy?: boolean;
    onToggle: (server: McpServerDto, enabled: boolean) => void;
  } = $props();

  let chip = $state<Chip>("all");

  const pool = $derived(filterMcpList(tab, filterQuery));
  const servers = $derived(
    chip === "on" ? pool.filter((server) => server.enabled) : chip === "off" ? pool.filter((server) => !server.enabled) : pool,
  );
  const live = $derived(tab.mcpServers.filter((server) => server.enabled).length);
  const hasProject = $derived(tab.mcpServers.some((server) => isProjectOrigin(server.origin)));
</script>

<div class="flex flex-col gap-3.5 px-5 pt-[18px] pb-[26px]">
  <header class="flex items-baseline gap-3">
    <h2 class="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">MCP servers</h2>
    <span class="font-mono text-xs leading-snug text-[var(--mute)]"
      >{live} live · {hasProject ? "global + this project" : "user-scope config only"} · handshake not probed</span
    >
    <div class="flex-1"></div>
    <div class="flex border border-[var(--hair)]" role="group" aria-label="Filter list">
      {#each ["all", "on", "off"] as next (next)}
        <button
          type="button"
          class="h-[26px] rounded-none border-0 px-3 text-[10.5px] font-semibold tracking-[0.03em] uppercase {chip === next
            ? 'bg-[var(--well)] text-[var(--silkscreen)]'
            : 'bg-transparent text-[var(--mute)]'}"
          aria-pressed={chip === next}
          onclick={() => (chip = next as Chip)}
        >
          {next}
        </button>
      {/each}
    </div>
  </header>

  {#if tab.mcpServers.length === 0}
    <p class="text-[13px] text-[var(--mute)]">{filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyMcps}</p>
  {:else if pool.length === 0}
    <p class="text-[13px] text-[var(--mute)]">{copy.filterMiss(filterQuery)}</p>
  {:else if servers.length === 0}
    <p class="text-[13px] text-[var(--mute)]">{chip === "on" ? "No MCP servers on." : "No MCP servers off."}</p>
  {:else}
    <div class="flex flex-col gap-1.5">
      {#each servers as server (server.id)}
        <article class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
          <div class="flex items-center gap-3 px-3 py-[11px]">
            <span class="flex min-h-6 min-w-6 shrink-0 items-center justify-center text-[var(--mute)]" aria-hidden="true">·</span>
            <span
              class="size-2 shrink-0 rounded-full {server.enabled
                ? 'bg-[var(--live)] shadow-[0_0_7px_var(--live)]'
                : 'bg-[var(--mute)]'}"
              aria-hidden="true"
            ></span>
            <div class="w-[238px] min-w-0 shrink-0">
              <div class="flex items-baseline gap-2">
                <span class="text-[16px]/[1.15] font-semibold break-words">{server.name}</span>
                <span
                  class="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--mute)]"
                  >{server.system.toUpperCase()}</span
                >
                {#if isProjectOrigin(server.origin)}
                  <span
                    class="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--mute)]"
                    >PROJECT</span
                  >
                {/if}
              </div>
              <div class="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]" title={server.source}>{server.source}</div>
            </div>
            <div class="ml-auto shrink-0">
              <Rocker
                size="plugin"
                on={server.enabled}
                {busy}
                disabled={!server.togglable}
                ariaLabel={`${server.name} ${server.enabled ? "on" : "off"}`}
                onToggle={() => onToggle(server, !server.enabled)}
              />
            </div>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>
