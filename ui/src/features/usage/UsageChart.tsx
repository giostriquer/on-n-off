import { useMemo } from "react";
import { PROVIDERS, providerLabel, type FoldedUsage } from "$lib/usageMerge";
import { buildChartSeries } from "$lib/usageChart";
import { formatTokens, formatUsd } from "$lib/usageFormat";
import type { UsageMetric } from "$lib/usageTypes";
import type { AgentId } from "$lib/types";

type UsageChartProps = {
  folded: FoldedUsage;
  metric: UsageMetric;
  sinceDay: string;
  untilDay: string;
  sinceTime?: string;
  untilTime?: string;
  hourly?: boolean;
};

function bandColor(provider: AgentId): string {
  if (provider === "claude") return "var(--usage-a)";
  if (provider === "codex") return "var(--usage-b)";
  return "var(--mute)";
}

export function UsageChart({
  folded,
  metric,
  sinceDay,
  untilDay,
  sinceTime,
  untilTime,
  hourly = false,
}: UsageChartProps) {
  const series = useMemo(
    () =>
      buildChartSeries({
        folded,
        metric,
        sinceDay,
        untilDay,
        sinceTime,
        untilTime,
        hourly,
      }),
    [folded, hourly, metric, sinceDay, sinceTime, untilDay, untilTime],
  );

  const height = 140;
  const padTop = 8;
  const padBottom = 22;
  const plotH = height - padTop - padBottom;
  const preferReduced =
    typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const labelStep = Math.max(1, Math.floor(series.columns.length / 6));

  function formatTotal(value: number): string {
    return metric === "cost" ? formatUsd(value) : formatTokens(value);
  }

  function stackRects(
    col: (typeof series.columns)[number],
    x: number,
    barW: number,
    usable: number,
  ) {
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

  return (
    <section className="usage-chart rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5" aria-label="Usage over time">
      <div className="mb-2 flex items-center gap-3">
        <span className="text-[11.5px] font-semibold tracking-[0.03em] uppercase">Activity</span>
        <div className="flex-1" />
        {PROVIDERS.map((provider) => (
          <span key={provider} className="flex items-center gap-1.5 font-mono text-[10.5px] text-[var(--mute)] uppercase">
            <span className="size-2 rounded-[1px]" style={{ background: bandColor(provider) }} aria-hidden="true" />
            {providerLabel(provider)}
          </span>
        ))}
      </div>

      {series.columns.every((c) => c.total === 0) ? (
        <p className="py-6 text-center text-[13px] text-[var(--mute)]">No activity in this window.</p>
      ) : (
        <svg
          className="block w-full"
          viewBox={`0 0 ${Math.max(series.columns.length * 28, 280)} ${height}`}
          role="img"
          aria-label={metric === "cost" ? "Stacked cost by provider" : "Stacked tokens by provider"}
        >
          {series.columns.map((col, index) => {
            const x = index * 28 + 4;
            const barW = 18;
            const usable = series.max || 1;
            const rects = stackRects(col, x, barW, usable);
            return (
              <g key={col.key}>
                {rects.map((rect) => (
                  <rect
                    key={rect.provider}
                    x={rect.x}
                    y={rect.y}
                    width={rect.width}
                    height={rect.height}
                    fill={bandColor(rect.provider)}
                    className={preferReduced ? undefined : "usage-bar"}
                  />
                ))}
                {index % labelStep === 0 || index === series.columns.length - 1 ? (
                  <text
                    x={x + barW / 2}
                    y={height - 6}
                    textAnchor="middle"
                    style={{
                      fontSize: 9,
                      fill: "var(--mute)",
                      fontFamily: "var(--font-mono), monospace",
                    }}
                  >
                    {col.label}
                  </text>
                ) : null}
                <title>
                  {col.label}: {formatTotal(col.total)}
                </title>
              </g>
            );
          })}
        </svg>
      )}
      <style>{`
        .usage-bar { transition: height 120ms ease, y 120ms ease; }
        @media (prefers-reduced-motion: reduce) {
          .usage-bar { transition: none; }
        }
      `}</style>
    </section>
  );
}
