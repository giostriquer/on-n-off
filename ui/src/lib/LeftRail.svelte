<script lang="ts">
  import Rocker from "./Rocker.svelte";
  import type { CatalogCounts, Screen } from "./catalog";

  let {
    screen,
    counts,
    masterOn,
    masterNote,
    busy = false,
    masterDisabled = false,
    showMasterCut = false,
    onScreen,
    onMaster,
  }: {
    screen: Screen;
    counts: CatalogCounts;
    masterOn: boolean;
    masterNote: string;
    busy?: boolean;
    masterDisabled?: boolean;
    showMasterCut?: boolean;
    onScreen: (next: Screen) => void;
    onMaster: (enabled: boolean) => void;
  } = $props();

  const items = $derived([
    { id: "overview" as const, label: "Overview", count: "" },
    { id: "plugins" as const, label: "Plugins", count: `${counts.plugins.on}/${counts.plugins.total}` },
    { id: "skills" as const, label: "Skills", count: `${counts.skills.on}/${counts.skills.total}` },
    { id: "mcp" as const, label: "MCP servers", count: `${counts.mcp.on}/${counts.mcp.total}` },
    { id: "usage" as const, label: "Usage", count: "" },
    { id: "config" as const, label: "Agent config", count: "" },
  ]);
</script>

<nav
  class="flex w-[186px] shrink-0 flex-col gap-0.5 border-r border-[var(--hair)] bg-[var(--plate)] px-2.5 py-3"
  aria-label="Section"
>
  {#each items as item (item.id)}
    {@const active = screen === item.id}
    <button
      type="button"
      class="flex h-[34px] items-center gap-2.5 rounded-none border-0 px-2 text-left text-[11.5px] font-semibold tracking-[0.04em] uppercase {active
        ? 'bg-[var(--well)] text-[var(--silkscreen)]'
        : 'bg-transparent text-[var(--mute)]'}"
      aria-current={active ? "page" : undefined}
      onclick={() => onScreen(item.id)}
    >
      <span class="h-[18px] w-[3px] shrink-0 {active ? 'bg-[var(--fill)]' : 'bg-transparent'}"></span>
      <span class="flex-1">{item.label}</span>
      {#if item.count}
        <span class="font-mono text-[11px] opacity-75">{item.count}</span>
      {/if}
    </button>
  {/each}
  <div class="flex-1"></div>
  {#if showMasterCut}
    <div class="flex flex-col gap-1.5 border border-dashed border-[var(--hair)] p-2.5">
      <span class="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">Master cut</span>
      <Rocker
        size="master"
        on={masterOn}
        dangerOff={!masterOn}
        busy={busy}
        disabled={masterDisabled}
        ariaLabel="Master cut"
        onToggle={() => onMaster(!masterOn)}
      />
      <span class="font-mono text-[10.5px] leading-snug text-[var(--mute)]">{masterNote}</span>
    </div>
  {/if}
</nav>
