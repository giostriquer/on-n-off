<script lang="ts">
  import { PROVIDERS, providerLabel, type FoldedUsage } from "./usageMerge";
  import { buildChartSeries } from "./usageChart";
  import { formatTokens, formatUsd } from "./usageFormat";
  import type { UsageMetric } from "./usageTypes";
  import type { AgentId } from "./types";

  let {
    folded,
    metric,
    sinceDay,
    untilDay,
    sinceTime = undefined,
    untilTime = undefined,
    hourly = false,
  }: {
    folded: FoldedUsage;
    metric: UsageMetric;
    sinceDay: string;
    untilDay: string;
    sinceTime?: string;
    untilTime?: string;
    hourly?: boolean;
  } = $props();

  const series = $derived(
    buildChartSeries({
      folded,
      metric,
      sinceDay,
      untilDay,
      sinceTime,
      untilTime,
      hourly,
    }),
  );

  const height = 140;
  const padTop = 8;
  const padBottom = 22;
  const plotH = height - padTop - padBottom;

  function bandColor(provider: AgentId): string {
    if (provider === "claude") return "var(--usage-a)";
    if (provider === "codex") return "var(--usage-b)";
    return "var(--mute)";
  }

  function formatTotal(value: number): string {
    return metric === "cost" ? formatUsd(value) : formatTokens(value);
  }

  function stackRects(col: (typeof series.columns)[number], x: number, barW: number, usable: number) {
    const yBase = padTop + plotH;
    let y = yBase;
    return col.bands
      .filter((band) => band.value > 0)
      .map((band) => {
        const h = (band.value / usable) * plotH;
        y -= h;
        return { provider: band.provider, x, y, width: barW, height: h };
      });
  }

  const preferReduced = $derived(
    typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches,
  );

  const labelStep = $derived(Math.max(1, Math.floor(series.columns.length / 6)));
</script>

<section class="usage-chart rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5" aria-label="Usage over time">
  <div class="mb-2 flex items-center gap-3">
    <span class="text-[11.5px] font-semibold tracking-[0.03em] uppercase">Activity</span>
    <div class="flex-1"></div>
    {#each PROVIDERS as provider (provider)}
      <span class="flex items-center gap-1.5 font-mono text-[10.5px] text-[var(--mute)] uppercase">
        <span class="size-2 rounded-[1px]" style:background={bandColor(provider)} aria-hidden="true"></span>
        {providerLabel(provider)}
      </span>
    {/each}
  </div>

  {#if series.columns.every((c) => c.total === 0)}
    <p class="py-6 text-center text-[13px] text-[var(--mute)]">No activity in this window.</p>
  {:else}
    <svg
      class="block w-full"
      viewBox={`0 0 ${Math.max(series.columns.length * 28, 280)} ${height}`}
      role="img"
      aria-label={metric === "cost" ? "Stacked cost by provider" : "Stacked tokens by provider"}
    >
      {#each series.columns as col, index (col.key)}
        {@const x = index * 28 + 4}
        {@const barW = 18}
        {@const usable = series.max || 1}
        {@const rects = stackRects(col, x, barW, usable)}
        <g>
          {#each rects as rect (rect.provider)}
            <rect
              x={rect.x}
              y={rect.y}
              width={rect.width}
              height={rect.height}
              fill={bandColor(rect.provider)}
              class={preferReduced ? "" : "usage-bar"}
            />
          {/each}
          {#if index % labelStep === 0 || index === series.columns.length - 1}
            <text
              x={x + barW / 2}
              y={height - 6}
              text-anchor="middle"
              style="font-size: 9px; fill: var(--mute); font-family: var(--font-mono), monospace;"
            >{col.label}</text>
          {/if}
          <title>{col.label}: {formatTotal(col.total)}</title>
        </g>
      {/each}
    </svg>
  {/if}
</section>

<style>
  .usage-bar {
    transition: height 120ms ease, y 120ms ease;
  }

  @media (prefers-reduced-motion: reduce) {
    .usage-bar {
      transition: none;
    }
  }
</style>
