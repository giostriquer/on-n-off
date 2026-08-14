import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import { foldUsage, providerLabel } from "$lib/usageMerge";
import {
  formatCount,
  formatDayShort,
  formatPercent,
  formatTokens,
  formatUsd,
  makeWindow,
} from "$lib/usageFormat";
import type { UsageMetric } from "$lib/usageTypes";
import { UsageChart } from "./UsageChart";

const WINDOWS = [
  { days: 1, label: "Past 24h" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
  { days: 90, label: "90 days" },
] as const;

export function Usage() {
  const [windowDays, setWindowDays] = useState(30);
  const [metric, setMetric] = useState<UsageMetric>("cost");
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
  const headline = metric === "cost" ? formatUsd(folded.costUsd) : formatTokens(folded.totalTokens);
  const recent = [...(windowDays === 1 ? folded.hourly : folded.daily)].reverse().slice(0, 8);
  const models = [...folded.models].sort((a, b) =>
    metric === "cost" ? b.costUsd - a.costUsd : b.totalTokens - a.totalTokens,
  );
  const bothMissing = !!summary && summary.sources.every((s) => s.status === "missing");
  const pricingNote = !summary
    ? ""
    : summary.pricing.status === "unavailable"
      ? "Token counts only · pricing table unavailable"
      : "API-equivalent estimate · not subscription billing";
  const showBody = !loading || !!summary;

  function changeWindow(days: number) {
    setWindowDays(days);
  }

  function refresh() {
    forceRef.current = true;
    setForceRevision((n) => n + 1);
  }

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]">
      <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5" aria-label="Usage controls">
        <div className="flex flex-wrap items-center gap-2">
          <div className="inline-grid grid-flow-col overflow-hidden rounded-lg border border-[var(--hair)]" role="group" aria-label="Window">
            {WINDOWS.map((option) => (
              <button
                key={option.days}
                type="button"
                className={`h-[30px] cursor-pointer rounded-none border-0 px-3 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] ${
                  windowDays === option.days
                    ? "bg-[var(--fill)] text-[var(--fill-ink)]"
                    : "bg-[var(--plate)] text-[var(--mute)]"
                }`}
                aria-pressed={windowDays === option.days}
                onClick={() => changeWindow(option.days)}
              >
                {option.label}
              </button>
            ))}
          </div>
          <div className="inline-grid grid-flow-col overflow-hidden rounded-lg border border-[var(--hair)]" role="group" aria-label="Metric">
            <button
              type="button"
              className={`h-[30px] cursor-pointer rounded-none border-0 px-3 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] ${
                metric === "cost" ? "bg-[var(--fill)] text-[var(--fill-ink)]" : "bg-[var(--plate)] text-[var(--mute)]"
              }`}
              aria-pressed={metric === "cost"}
              onClick={() => setMetric("cost")}
            >
              Cost
            </button>
            <button
              type="button"
              className={`h-[30px] cursor-pointer rounded-none border-0 px-3 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] ${
                metric === "tokens" ? "bg-[var(--fill)] text-[var(--fill-ink)]" : "bg-[var(--plate)] text-[var(--mute)]"
              }`}
              aria-pressed={metric === "tokens"}
              onClick={() => setMetric("tokens")}
            >
              Tokens
            </button>
          </div>
          <div className="flex-1" />
          <button
            type="button"
            className="flex h-[30px] items-center gap-1.5 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-2.5 text-[11px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fill)] disabled:opacity-45"
            disabled={loading}
            onClick={refresh}
          >
            <RefreshCw className={`size-3 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
            Refresh
          </button>
        </div>
      </section>

      {loading && !summary ? (
        <p className="text-[13px] text-[var(--mute)]">Scanning transcripts…</p>
      ) : error && !summary ? (
        <p className="text-[13px] text-[var(--trip)]">{error}</p>
      ) : bothMissing ? (
        <p className="text-[13px] text-[var(--mute)]">No Claude or Codex session transcripts found on this machine.</p>
      ) : showBody ? (
        <>
          {error ? <p className="text-[13px] text-[var(--trip)]">{error}</p> : null}
          <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-4" aria-label="Usage totals">
            <div className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
              {metric === "cost" ? "Estimated cost" : "Processed tokens"}
            </div>
            <div className="mt-2 text-[34px] leading-none font-semibold">{headline}</div>
            <div className="mt-2.5 font-mono text-[11.5px] text-[var(--mute)]">
              {formatCount(folded.sessions)} sessions · {formatCount(folded.records)} records
              {folded.cacheSavingsUsd > 0 && metric === "cost" ? ` · cache saved ${formatUsd(folded.cacheSavingsUsd)}` : ""}
              {loading ? " · scanning…" : summary?.cacheHit ? " · cached" : ""}
            </div>
            <p className="mt-3 text-[11.5px] text-[var(--mute)]">{pricingNote}</p>
            {summary ? (
              <div className="mt-3 flex flex-wrap gap-2">
                {summary.sources.map((source) => (
                  <span
                    key={source.provider}
                    className={`rounded border border-[var(--hair)] px-2 py-1 font-mono text-[10.5px] tracking-[0.03em] uppercase ${
                      source.status === "ok" ? "text-[var(--silkscreen)]" : "text-[var(--mute)]"
                    }`}
                  >
                    {providerLabel(source.provider)}{" "}
                    {source.status === "ok" ? "ok" : source.status === "missing" ? "missing" : source.status}
                  </span>
                ))}
              </div>
            ) : null}
          </section>

          <UsageChart
            folded={folded}
            metric={metric}
            sinceDay={activeWindow.sinceDay}
            untilDay={activeWindow.untilDay}
            sinceTime={activeWindow.sinceTime}
            untilTime={activeWindow.untilTime}
            hourly={windowDays === 1}
          />

          <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Models">
            <header className="border-b border-[var(--hair)] px-3 py-[9px] text-[11.5px] font-semibold tracking-[0.03em] uppercase">
              Models
            </header>
            {models.length === 0 ? (
              <p className="px-3 py-3 text-[13px] text-[var(--mute)]">No model activity in this window.</p>
            ) : (
              models.map((row) => (
                <div key={`${row.provider}-${row.model}`} className="flex items-center gap-3 border-b border-[var(--hair)] px-3 py-2.5 last:border-b-0">
                  <span className="w-[52px] shrink-0 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
                    {providerLabel(row.provider)}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[14px] font-semibold leading-none">{row.model}</span>
                  <span className="font-mono text-[12px] text-[var(--mute)]">
                    {formatPercent(metric === "cost" ? row.costShare : row.tokenShare)}
                  </span>
                  <span className="font-mono w-[88px] text-right text-[13px] font-medium">
                    {metric === "cost" ? formatUsd(row.costUsd) : formatTokens(row.totalTokens)}
                  </span>
                </div>
              ))
            )}
          </section>

          <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Recent periods">
            <header className="border-b border-[var(--hair)] px-3 py-[9px] text-[11.5px] font-semibold tracking-[0.03em] uppercase">
              Recent
            </header>
            {recent.length === 0 ? (
              <p className="px-3 py-3 text-[13px] text-[var(--mute)]">No activity in this window.</p>
            ) : (
              recent.map((period) => (
                <div
                  key={period.hourStart ?? period.day}
                  className="flex items-center gap-3 border-b border-[var(--hair)] px-3 py-2.5 last:border-b-0"
                >
                  <span className="min-w-[88px] font-mono text-[12px] text-[var(--mute)]">
                    {period.hourStart
                      ? new Date(period.hourStart).toLocaleString(undefined, {
                          month: "short",
                          day: "numeric",
                          hour: "numeric",
                        })
                      : formatDayShort(period.day)}
                  </span>
                  <div className="flex-1" />
                  <span className="font-mono text-[13px] font-medium">
                    {metric === "cost" ? formatUsd(period.costUsd) : formatTokens(period.totalTokens)}
                  </span>
                </div>
              ))
            )}
          </section>
        </>
      ) : null}
    </div>
  );
}
