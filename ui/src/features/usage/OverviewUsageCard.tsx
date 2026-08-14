import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import { ProviderIcon } from "$lib/ProviderIcon";
import { foldUsage, providerLabel } from "$lib/usageMerge";
import { formatDayRange, formatPercent, formatTokens, formatUsd, makeWindow } from "$lib/usageFormat";
import type { UsageMetric } from "$lib/usageTypes";
import { UsageChart } from "@/features/usage/UsageChart";

/** Compact Overview panel — links through to the full Usage screen. */
export function OverviewUsageCard() {
  const [metric, setMetric] = useState<UsageMetric>("cost");
  const window = makeWindow(30);

  const query = useQuery({
    queryKey: ["usage", 30, 0],
    queryFn: () =>
      api.usageSummary({
        sinceDay: window.sinceDay,
        untilDay: window.untilDay,
        timeZone: window.timeZone,
        resolution: window.resolution,
        sinceTime: window.sinceTime,
        untilTime: window.untilTime,
      }),
  });

  const summary = query.data ?? null;
  const loading = query.isFetching;
  const error = query.error ? displayError(parseInvokeError(query.error), "Usage") : null;
  const folded = foldUsage(summary);
  const bothMissing = !!summary && summary.sources.every((s) => s.status === "missing");

  const models = [...folded.models]
    .sort((a, b) => (metric === "cost" ? b.costUsd - a.costUsd : b.totalTokens - a.totalTokens))
    .slice(0, 4);

  const pricingNote = !summary
    ? ""
    : summary.pricing.status === "unavailable"
      ? "Token counts only · pricing table unavailable"
      : "if billed at full API rate";

  return (
    <section
      className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label="Usage summary"
    >
      <header className="flex items-center gap-2.5 border-b border-[var(--hair)] px-3.5 py-2.5">
        <span className="text-[11.5px] font-semibold tracking-[0.03em] uppercase">Usage</span>
        <span className="font-mono text-[11px] text-[var(--mute)]">
          {formatDayRange(window.sinceDay, window.untilDay)}
          {loading ? " · scanning…" : null}
        </span>
        <div className="flex-1" />
        <Link
          to="/usage"
          className="text-[11px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase no-underline hover:text-[var(--silkscreen)]"
        >
          Open usage
        </Link>
      </header>

      {loading && !summary ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">Scanning transcripts…</p>
      ) : error && !summary ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--trip)]">{error}</p>
      ) : bothMissing ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">
          No Claude or Codex session transcripts found on this machine.{" "}
          <Link to="/usage" className="text-[var(--silkscreen)] underline-offset-2 hover:underline">
            Open usage
          </Link>
        </p>
      ) : (
        <>
          {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}
          <div className="grid md:grid-cols-[236px_minmax(0,1fr)]">
            <div className="flex flex-col gap-2 border-b border-[var(--hair)] p-3.5 md:border-r md:border-b-0">
              <span className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
                {metric === "cost" ? "Raw token cost" : "Processed tokens"}
              </span>
              <span className="text-[34px] leading-none font-semibold tracking-[-0.03em]">
                {metric === "cost" ? formatUsd(folded.costUsd) : formatTokens(folded.totalTokens)}
              </span>
              <span className="font-mono text-[11.5px] leading-snug text-[var(--mute)]">
                {formatTokens(folded.totalTokens)} tokens · {folded.activeDays} active days
              </span>
              {pricingNote ? <span className="text-[11.5px] leading-snug text-[var(--mute)]">{pricingNote}</span> : null}
              <span className="font-mono text-[10.5px] leading-snug text-[var(--mute)]">
                machine-wide · session transcripts · not scoped to a folder
              </span>
              <div className="mt-auto flex flex-wrap gap-1.5 pt-2">
                {(summary?.sources ?? []).map((source) => (
                  <span
                    key={source.provider}
                    className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 font-mono text-[10px] uppercase ${
                      source.status === "ok"
                        ? "border-[var(--hair)] text-[var(--silkscreen)]"
                        : "border-[var(--hair)] text-[var(--mute)]"
                    }`}
                  >
                    <ProviderIcon provider={source.provider} className="size-3" />
                    {providerLabel(source.provider)} · {source.status}
                  </span>
                ))}
              </div>
            </div>
            <div className="min-w-0 p-3">
              <UsageChart
                folded={folded}
                metric={metric}
                onMetricChange={setMetric}
                sinceDay={window.sinceDay}
                untilDay={window.untilDay}
                sinceTime={window.sinceTime}
                untilTime={window.untilTime}
              />
            </div>
          </div>
          <div className="border-t border-[var(--hair)]">
            <div className="px-3.5 pt-2.5 text-[10.5px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
              Models
            </div>
            {models.length === 0 ? (
              <p className="px-3.5 py-3 text-[13px] text-[var(--mute)]">No model activity in this window.</p>
            ) : (
              models.map((row) => {
                const share = metric === "cost" ? row.costShare : row.tokenShare;
                return (
                  <div
                    key={`${row.provider}-${row.model}`}
                    className="flex items-center gap-2.5 border-b border-[var(--hair)] px-3.5 py-2 last:border-b-0"
                  >
                    <span className="w-[52px] shrink-0 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
                      {providerLabel(row.provider)}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[14px] font-semibold">{row.model}</span>
                    <span className="h-1 w-24 shrink-0 overflow-hidden rounded-sm bg-[var(--well)]">
                      <span
                        className="block h-full rounded-sm bg-[var(--fill)]"
                        style={{ width: `${Math.max(0, Math.min(100, share * 100))}%` }}
                      />
                    </span>
                    <span className="font-mono w-16 shrink-0 text-right text-[12px]">
                      {metric === "cost" ? formatUsd(row.costUsd) : formatTokens(row.totalTokens)}
                    </span>
                    <span className="font-mono w-10 shrink-0 text-right text-[11px] text-[var(--mute)]">
                      {formatPercent(share)}
                    </span>
                  </div>
                );
              })
            )}
          </div>
        </>
      )}
    </section>
  );
}
