import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import { PROVIDERS, foldUsage, providerLabel } from "$lib/usageMerge";
import {
  formatCount,
  formatDayRange,
  formatDayShort,
  formatPercent,
  formatTokens,
  formatUsd,
  FULL_TIME_DAYS,
  makeWindow,
} from "$lib/usageFormat";
import type { UsageMetric } from "$lib/usageTypes";
import type { AgentId } from "$lib/types";
import { ProviderIcon } from "$lib/ProviderIcon";
import { UsageChart } from "./UsageChart";

const WINDOWS = [
  { days: 1, label: "Past 24h" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
  { days: 90, label: "90 days" },
  { days: FULL_TIME_DAYS, label: "Full time" },
] as const;

type BreakdownMode = "model" | "day";

const PROVIDER_DOT: Record<AgentId, string> = {
  codex: "var(--silkscreen)",
  claude: "#e8944a",
  antigravity: "var(--mute)",
};

function Segmented<T extends string>({
  value,
  options,
  ariaLabel,
  onChange,
}: {
  value: T;
  options: readonly { value: T; label: string }[];
  ariaLabel: string;
  onChange: (value: T) => void;
}) {
  return (
    <div
      className="inline-grid grid-flow-col overflow-hidden rounded-md border border-[var(--hair)]"
      role="group"
      aria-label={ariaLabel}
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={`h-7 cursor-pointer rounded-none border-0 px-2.5 text-[10.5px] font-semibold tracking-[0.05em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] ${
            value === option.value
              ? "bg-[var(--fill)] text-[var(--fill-ink)]"
              : "bg-transparent text-[var(--mute)]"
          }`}
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Usage() {
  const [windowDays, setWindowDays] = useState(30);
  const [metric, setMetric] = useState<UsageMetric>("cost");
  const [breakdown, setBreakdown] = useState<BreakdownMode>("model");
  const [forceRevision, setForceRevision] = useState(0);
  const forceRef = useRef(false);
  const activeWindow = makeWindow(windowDays);

  const query = useQuery({
    queryKey: ["usage", windowDays, forceRevision],
    queryFn: async () => {
      const window = makeWindow(windowDays);
      const force = forceRef.current;
      forceRef.current = false;
      return api.usageSummary({
        sinceDay: window.sinceDay,
        untilDay: window.untilDay,
        timeZone: window.timeZone,
        resolution: window.resolution,
        sinceTime: window.sinceTime,
        untilTime: window.untilTime,
        force,
      });
    },
  });

  const summary = query.data ?? null;
  const loading = query.isFetching;
  const error = query.error ? displayError(parseInvokeError(query.error), "Usage") : null;
  const folded = foldUsage(summary);
  const bothMissing = !!summary && summary.sources.every((s) => s.status === "missing");
  const showBody = !loading || !!summary;

  const inputTokens =
    folded.tokens.uncachedInputTokens + folded.tokens.cachedInputTokens + folded.tokens.cacheCreationTokens;
  const cachedShare = inputTokens === 0 ? 0 : folded.tokens.cachedInputTokens / inputTokens;
  const tokensPerActiveDay = folded.activeDays === 0 ? 0 : folded.totalTokens / folded.activeDays;
  const cacheMultiple = folded.costUsd > 0 ? folded.cacheSavingsUsd / folded.costUsd : 0;

  const providers = PROVIDERS.map((provider) => {
    const row = folded.providers.find((item) => item.provider === provider);
    return (
      row ?? {
        provider,
        costUsd: 0,
        totalTokens: 0,
        records: 0,
        costShare: 0,
        tokenShare: 0,
      }
    );
  }).sort((a, b) => b.costUsd - a.costUsd || b.totalTokens - a.totalTokens);

  const models = [...folded.models].sort((a, b) =>
    metric === "cost" ? b.costUsd - a.costUsd : b.totalTokens - a.totalTokens,
  );

  const days = [...folded.daily]
    .filter((period) => period.costUsd > 0 || period.totalTokens > 0)
    .sort((a, b) =>
      metric === "cost" ? b.costUsd - a.costUsd : b.totalTokens - a.totalTokens,
    );

  const pricingNote = !summary
    ? ""
    : summary.pricing.status === "unavailable"
      ? "Token counts only · pricing table unavailable"
      : "if billed at full API rate";

  const activityDays = [...folded.daily]
    .filter((period) => period.costUsd > 0 || period.totalTokens > 0)
    .map((period) => period.day)
    .sort();
  const rangeSince =
    windowDays === FULL_TIME_DAYS && activityDays.length > 0 ? activityDays[0]! : activeWindow.sinceDay;
  const rangeUntil =
    windowDays === FULL_TIME_DAYS && activityDays.length > 0
      ? activityDays[activityDays.length - 1]!
      : activeWindow.untilDay;

  function refresh() {
    forceRef.current = true;
    setForceRevision((n) => n + 1);
  }

  return (
    <div className="flex flex-col gap-7 px-5 pt-[18px] pb-[28px]">
      <header className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1 font-mono text-[12px] text-[var(--mute)]">
          {windowDays === FULL_TIME_DAYS ? "Full time · " : null}
          {formatDayRange(rangeSince, rangeUntil)}
          {loading ? <span className="ml-2">· scanning…</span> : null}
          {!loading && summary?.cacheHit ? <span className="ml-2">· cached</span> : null}
        </div>
        <Segmented
          ariaLabel="Window"
          value={String(windowDays)}
          options={WINDOWS.map((option) => ({ value: String(option.days), label: option.label }))}
          onChange={(value) => setWindowDays(Number(value))}
        />
        <button
          type="button"
          className="inline-flex size-7 items-center justify-center rounded-md border border-[var(--hair)] bg-transparent text-[var(--mute)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fill)] disabled:opacity-45"
          disabled={loading}
          aria-label="Refresh usage"
          onClick={refresh}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
        </button>
      </header>

      {loading && !summary ? (
        <p className="text-[13px] text-[var(--mute)]">Scanning transcripts…</p>
      ) : error && !summary ? (
        <p className="text-[13px] text-[var(--trip)]">{error}</p>
      ) : bothMissing ? (
        <p className="text-[13px] text-[var(--mute)]">No Claude or Codex session transcripts found on this machine.</p>
      ) : showBody ? (
        <>
          {error ? <p className="text-[13px] text-[var(--trip)]">{error}</p> : null}

          <section aria-label="Usage totals">
            <div className="text-[11px] font-semibold tracking-[0.06em] text-[var(--mute)] uppercase">
              {metric === "cost" ? "Raw token cost" : "Processed tokens"}
            </div>
            <div className="mt-1.5 text-[40px] leading-none font-semibold tracking-[-0.03em]">
              {metric === "cost" ? formatUsd(folded.costUsd) : formatTokens(folded.totalTokens)}
            </div>
            <p className="mt-2 text-[12px] text-[var(--mute)]">{pricingNote}</p>

            <div className="mt-5 flex flex-col gap-3.5">
              {providers.map((row) => {
                const share = metric === "cost" ? row.costShare : row.tokenShare;
                return (
                  <div key={row.provider} className="grid gap-1.5">
                    <div className="flex items-center gap-2">
                      <ProviderIcon provider={row.provider} className="size-3.5 shrink-0" />
                      <span className="text-[13px] font-semibold">{providerLabel(row.provider)}</span>
                      <span className="font-mono text-[13px] font-medium">
                        {metric === "cost" ? formatUsd(row.costUsd) : formatTokens(row.totalTokens)}
                      </span>
                      <span className="font-mono text-[11.5px] text-[var(--mute)]">
                        {formatPercent(share)} of {metric}
                        {" · "}
                        {formatTokens(row.totalTokens)} tokens
                      </span>
                    </div>
                    <div className="h-[3px] overflow-hidden rounded-full bg-[var(--well)]">
                      <div
                        className="h-full rounded-full"
                        style={{
                          width: `${Math.max(0, Math.min(100, share * 100))}%`,
                          background: PROVIDER_DOT[row.provider],
                        }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          </section>

          <UsageChart
            folded={folded}
            metric={metric}
            onMetricChange={setMetric}
            sinceDay={activeWindow.sinceDay}
            untilDay={activeWindow.untilDay}
            sinceTime={activeWindow.sinceTime}
            untilTime={activeWindow.untilTime}
            hourly={windowDays === 1}
            sparse={windowDays === FULL_TIME_DAYS}
          />

          <section
            className="grid gap-4 border-y border-[var(--hair)] py-4 sm:grid-cols-2 lg:grid-cols-5"
            aria-label="Token metrics"
          >
            <Metric
              label="Processed tokens"
              value={formatTokens(folded.totalTokens)}
              hint={
                folded.activeDays > 0
                  ? `${formatTokens(tokensPerActiveDay)} per active day`
                  : `${formatCount(folded.sessions)} sessions`
              }
            />
            <Metric
              label="Cached input"
              value={formatTokens(folded.tokens.cachedInputTokens)}
              hint={`${formatPercent(cachedShare)} of observed input`}
            />
            <Metric
              label="Uncached input"
              value={formatTokens(folded.tokens.uncachedInputTokens)}
              hint={`${formatTokens(folded.tokens.cacheCreationTokens)} cache writes`}
            />
            <Metric
              label="Output"
              value={formatTokens(folded.tokens.outputTokens)}
              hint={
                folded.tokens.reasoningTokens > 0
                  ? `includes ${formatTokens(folded.tokens.reasoningTokens)} reasoning`
                  : `${formatCount(folded.records)} records`
              }
            />
            <Metric
              label="Cache savings"
              value={formatUsd(folded.cacheSavingsUsd)}
              hint={cacheMultiple > 0 ? `${cacheMultiple.toFixed(1)}x the raw token cost` : "vs full API rate"}
            />
          </section>

          <section aria-label="Breakdown">
            <div className="mb-3 flex items-center gap-3">
              <span className="text-[13px] font-semibold tracking-[-0.01em]">Breakdown</span>
              <div className="flex-1" />
              <Segmented
                ariaLabel="Breakdown"
                value={breakdown}
                options={[
                  { value: "model", label: "Model" },
                  { value: "day", label: "Day" },
                ]}
                onChange={setBreakdown}
              />
            </div>

            <div className="grid grid-cols-[minmax(0,1.4fr)_88px_64px_88px] gap-2 border-b border-[var(--hair)] pb-2 font-mono text-[10.5px] tracking-[0.04em] text-[var(--mute)] uppercase">
              <span>{breakdown === "model" ? "Model" : "Day"}</span>
              <span className="text-right">Cost</span>
              <span className="text-right">Share</span>
              <span className="text-right">Tokens</span>
            </div>

            {breakdown === "model" ? (
              models.length === 0 ? (
                <p className="py-4 text-[13px] text-[var(--mute)]">No model activity in this window.</p>
              ) : (
                models.map((row) => (
                  <div
                    key={`${row.provider}-${row.model}`}
                    className="grid grid-cols-[minmax(0,1.4fr)_88px_64px_88px] items-center gap-2 border-b border-[var(--hair)] py-2.5 last:border-b-0"
                  >
                    <div className="flex min-w-0 items-center gap-2">
                      <ProviderIcon provider={row.provider} className="size-3.5 shrink-0" />
                      <span className="truncate text-[13.5px] font-medium">{row.model}</span>
                    </div>
                    <span className="font-mono text-right text-[12.5px]">{formatUsd(row.costUsd)}</span>
                    <span className="font-mono text-right text-[12px] text-[var(--mute)]">
                      {formatPercent(metric === "cost" ? row.costShare : row.tokenShare)}
                    </span>
                    <span className="font-mono text-right text-[12.5px]">{formatTokens(row.totalTokens)}</span>
                  </div>
                ))
              )
            ) : days.length === 0 ? (
              <p className="py-4 text-[13px] text-[var(--mute)]">No activity in this window.</p>
            ) : (
              days.map((period) => {
                const share =
                  metric === "cost"
                    ? folded.costUsd === 0
                      ? 0
                      : period.costUsd / folded.costUsd
                    : folded.totalTokens === 0
                      ? 0
                      : period.totalTokens / folded.totalTokens;
                return (
                  <div
                    key={period.day}
                    className="grid grid-cols-[minmax(0,1.4fr)_88px_64px_88px] items-center gap-2 border-b border-[var(--hair)] py-2.5 last:border-b-0"
                  >
                    <span className="font-mono text-[13px]">{formatDayShort(period.day)}</span>
                    <span className="font-mono text-right text-[12.5px]">{formatUsd(period.costUsd)}</span>
                    <span className="font-mono text-right text-[12px] text-[var(--mute)]">{formatPercent(share)}</span>
                    <span className="font-mono text-right text-[12.5px]">{formatTokens(period.totalTokens)}</span>
                  </div>
                );
              })
            )}
          </section>
        </>
      ) : null}
    </div>
  );
}

function Metric({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="min-w-0">
      <div className="text-[10.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">{label}</div>
      <div className="mt-1 text-[18px] leading-none font-semibold tracking-[-0.02em]">{value}</div>
      <div className="mt-1.5 text-[11.5px] text-[var(--mute)]">{hint}</div>
    </div>
  );
}
