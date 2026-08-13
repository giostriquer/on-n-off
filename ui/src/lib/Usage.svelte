<script lang="ts">
  import { onMount } from "svelte";
  import RefreshCw from "lucide-svelte/icons/refresh-cw";
  import * as api from "./api";
  import { displayError, parseInvokeError } from "./error";
  import UsageChart from "./UsageChart.svelte";
  import { foldUsage, providerLabel } from "./usageMerge";
  import {
    formatCount,
    formatDayShort,
    formatPercent,
    formatTokens,
    formatUsd,
    makeWindow,
  } from "./usageFormat";
  import type { UsageMetric, UsageSummary } from "./usageTypes";

  const WINDOWS = [
    { days: 1, label: "Past 24h" },
    { days: 7, label: "7 days" },
    { days: 30, label: "30 days" },
    { days: 90, label: "90 days" },
  ] as const;

  let summary = $state<UsageSummary | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let windowDays = $state(30);
  let metric = $state<UsageMetric>("cost");
  let activeWindow = $state(makeWindow(30));
  let loadGen = 0;

  const folded = $derived(foldUsage(summary));
  const headline = $derived(metric === "cost" ? formatUsd(folded.costUsd) : formatTokens(folded.totalTokens));
  const recent = $derived(
    (windowDays === 1 ? folded.hourly : folded.daily).toReversed().slice(0, 8),
  );
  const models = $derived(
    [...folded.models].sort((a, b) =>
      metric === "cost" ? b.costUsd - a.costUsd : b.totalTokens - a.totalTokens,
    ),
  );
  const bothMissing = $derived(!!summary && summary.sources.every((s) => s.status === "missing"));
  const pricingNote = $derived(
    !summary
      ? ""
      : summary.pricing.status === "unavailable"
        ? "Token counts only · pricing table unavailable"
        : "API-equivalent estimate · not subscription billing",
  );
  const showBody = $derived(!loading || !!summary);

  onMount(() => {
    void load(false);
  });

  async function load(force: boolean) {
    const gen = ++loadGen;
    loading = true;
    error = null;
    const window = makeWindow(windowDays);
    activeWindow = window;
    try {
      const next = await api.usageSummary({
        sinceDay: window.sinceDay,
        untilDay: window.untilDay,
        timeZone: window.timeZone,
        resolution: window.resolution,
        sinceTime: window.sinceTime,
        untilTime: window.untilTime,
        force,
      });
      if (gen !== loadGen) {
        return;
      }
      summary = next;
    } catch (err) {
      if (gen !== loadGen) {
        return;
      }
      error = displayError(parseInvokeError(err));
    } finally {
      if (gen === loadGen) {
        loading = false;
      }
    }
  }

  function setWindowDays(days: number) {
    windowDays = days;
    void load(false);
  }
</script>

<div class="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]">
  <section class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5" aria-label="Usage controls">
    <div class="flex flex-wrap items-center gap-2">
      <div class="inline-grid grid-flow-col overflow-hidden rounded-lg border border-[var(--hair)]" role="group" aria-label="Window">
        {#each WINDOWS as option (option.days)}
          <button
            type="button"
            class="h-[30px] cursor-pointer rounded-none border-0 px-3 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] {windowDays ===
            option.days
              ? 'bg-[var(--fill)] text-[var(--fill-ink)]'
              : 'bg-[var(--plate)] text-[var(--mute)]'}"
            aria-pressed={windowDays === option.days}
            onclick={() => setWindowDays(option.days)}
          >
            {option.label}
          </button>
        {/each}
      </div>
      <div class="inline-grid grid-flow-col overflow-hidden rounded-lg border border-[var(--hair)]" role="group" aria-label="Metric">
        <button
          type="button"
          class="h-[30px] cursor-pointer rounded-none border-0 px-3 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] {metric ===
          'cost'
            ? 'bg-[var(--fill)] text-[var(--fill-ink)]'
            : 'bg-[var(--plate)] text-[var(--mute)]'}"
          aria-pressed={metric === "cost"}
          onclick={() => (metric = "cost")}
        >
          Cost
        </button>
        <button
          type="button"
          class="h-[30px] cursor-pointer rounded-none border-0 px-3 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] {metric ===
          'tokens'
            ? 'bg-[var(--fill)] text-[var(--fill-ink)]'
            : 'bg-[var(--plate)] text-[var(--mute)]'}"
          aria-pressed={metric === "tokens"}
          onclick={() => (metric = "tokens")}
        >
          Tokens
        </button>
      </div>
      <div class="flex-1"></div>
      <button
        type="button"
        class="flex h-[30px] items-center gap-1.5 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-2.5 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fill)] disabled:opacity-45"
        disabled={loading}
        onclick={() => void load(true)}
      >
        <RefreshCw class="size-3 {loading ? 'animate-spin' : ''}" aria-hidden="true" />
        Refresh
      </button>
    </div>
  </section>

  {#if loading && !summary}
    <p class="text-[13px] text-[var(--mute)]">Scanning transcripts…</p>
  {:else if error && !summary}
    <p class="text-[13px] text-[var(--trip)]">{error}</p>
  {:else if bothMissing}
    <p class="text-[13px] text-[var(--mute)]">No Claude or Codex session transcripts found on this machine.</p>
  {:else if showBody}
    {#if error}
      <p class="text-[13px] text-[var(--trip)]">{error}</p>
    {/if}
    <section class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-4" aria-label="Usage totals">
      <div class="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
        {metric === "cost" ? "Estimated cost" : "Processed tokens"}
      </div>
      <div class="mt-2 text-[34px] leading-none font-semibold">{headline}</div>
      <div class="mt-2.5 font-mono text-[11.5px] text-[var(--mute)]">
        {formatCount(folded.sessions)} sessions · {formatCount(folded.records)} records
        {#if folded.cacheSavingsUsd > 0 && metric === "cost"}
          · cache saved {formatUsd(folded.cacheSavingsUsd)}
        {/if}
        {#if loading}
          · scanning…
        {:else if summary?.cacheHit}
          · cached
        {/if}
      </div>
      <p class="mt-3 text-[11.5px] text-[var(--mute)]">{pricingNote}</p>
      {#if summary}
        <div class="mt-3 flex flex-wrap gap-2">
          {#each summary.sources as source (source.provider)}
            <span
              class="rounded border border-[var(--hair)] px-2 py-1 font-mono text-[10.5px] tracking-[0.03em] uppercase {source.status ===
              'ok'
                ? 'text-[var(--silkscreen)]'
                : 'text-[var(--mute)]'}"
            >
              {providerLabel(source.provider)}
              {#if source.status === "ok"}
                ok
              {:else if source.status === "missing"}
                missing
              {:else}
                {source.status}
              {/if}
            </span>
          {/each}
        </div>
      {/if}
    </section>

    <UsageChart
      {folded}
      {metric}
      sinceDay={activeWindow.sinceDay}
      untilDay={activeWindow.untilDay}
      sinceTime={activeWindow.sinceTime}
      untilTime={activeWindow.untilTime}
      hourly={windowDays === 1}
    />

    <section class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Models">
      <header class="border-b border-[var(--hair)] px-3 py-[9px] text-[11.5px] font-semibold tracking-[0.03em] uppercase">
        Models
      </header>
      {#if models.length === 0}
        <p class="px-3 py-3 text-[13px] text-[var(--mute)]">No model activity in this window.</p>
      {:else}
        {#each models as row (row.provider + row.model)}
          <div class="flex items-center gap-3 border-b border-[var(--hair)] px-3 py-2.5 last:border-b-0">
            <span class="w-[52px] shrink-0 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase"
              >{providerLabel(row.provider)}</span
            >
            <span class="min-w-0 flex-1 truncate text-[14px] font-semibold leading-none">{row.model}</span>
            <span class="font-mono text-[12px] text-[var(--mute)]"
              >{formatPercent(metric === "cost" ? row.costShare : row.tokenShare)}</span
            >
            <span class="font-mono w-[88px] text-right text-[13px] font-medium">
              {metric === "cost" ? formatUsd(row.costUsd) : formatTokens(row.totalTokens)}
            </span>
          </div>
        {/each}
      {/if}
    </section>

    <section class="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Recent periods">
      <header class="border-b border-[var(--hair)] px-3 py-[9px] text-[11.5px] font-semibold tracking-[0.03em] uppercase">
        Recent
      </header>
      {#if recent.length === 0}
        <p class="px-3 py-3 text-[13px] text-[var(--mute)]">No activity in this window.</p>
      {:else}
        {#each recent as period (period.hourStart ?? period.day)}
          <div class="flex items-center gap-3 border-b border-[var(--hair)] px-3 py-2.5 last:border-b-0">
            <span class="min-w-[88px] font-mono text-[12px] text-[var(--mute)]">
              {period.hourStart
                ? new Date(period.hourStart).toLocaleString(undefined, {
                    month: "short",
                    day: "numeric",
                    hour: "numeric",
                  })
                : formatDayShort(period.day)}
            </span>
            <div class="flex-1"></div>
            <span class="font-mono text-[13px] font-medium">
              {metric === "cost" ? formatUsd(period.costUsd) : formatTokens(period.totalTokens)}
            </span>
          </div>
        {/each}
      {/if}
    </section>
  {/if}
</div>
