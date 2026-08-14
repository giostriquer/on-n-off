import { useEffect, useMemo, useState } from "react";
import { areaY, defineChart, lineY } from "@tanstack/charts";
import { Chart } from "@tanstack/charts/react";
import { crosshair } from "@tanstack/charts/crosshair";
import { scaleLinear } from "@tanstack/charts/scales/linear";
import { scalePoint } from "@tanstack/charts/scales/point";
import { tooltip } from "@tanstack/charts/tooltip";
import { PROVIDERS, providerLabel, type FoldedUsage } from "$lib/usageMerge";
import { buildChartSeries, toChartRows } from "$lib/usageChart";
import { formatTokens, formatUsd } from "$lib/usageFormat";
import type { UsageMetric } from "$lib/usageTypes";
import type { AgentId } from "$lib/types";
import "./UsageChart.css";

export type UsageChartProps = {
  folded: FoldedUsage;
  metric: UsageMetric;
  onMetricChange: (metric: UsageMetric) => void;
  sinceDay: string;
  untilDay: string;
  sinceTime?: string;
  untilTime?: string;
  hourly?: boolean;
  /** Full-time windows only plot days with activity. */
  sparse?: boolean;
};

/** T3-like contrast: Codex near-white, Claude warm orange. */
const PROVIDER_COLORS: Record<"light" | "dark", Record<AgentId, string>> = {
  light: {
    codex: "#2a2a2a",
    claude: "#d9773a",
    antigravity: "#6c727c",
  },
  dark: {
    codex: "#ececec",
    claude: "#e8944a",
    antigravity: "#8c939d",
  },
};

function useDarkTheme(): boolean {
  const [dark, setDark] = useState(() =>
    typeof document !== "undefined" ? document.documentElement.classList.contains("dark") : true,
  );

  useEffect(() => {
    const root = document.documentElement;
    const sync = () => setDark(root.classList.contains("dark"));
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(root, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);

  return dark;
}

function bandColor(provider: AgentId, dark: boolean): string {
  return PROVIDER_COLORS[dark ? "dark" : "light"][provider] ?? PROVIDER_COLORS.dark.antigravity;
}

export function UsageChart({
  folded,
  metric,
  onMetricChange,
  sinceDay,
  untilDay,
  sinceTime,
  untilTime,
  hourly = false,
  sparse = false,
}: UsageChartProps) {
  const dark = useDarkTheme();
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
        sparse,
      }),
    [folded, hourly, metric, sinceDay, sinceTime, sparse, untilDay, untilTime],
  );

  const rows = useMemo(() => toChartRows(series), [series]);
  const empty = series.columns.every((column) => column.total === 0);
  const colors = useMemo(() => PROVIDERS.map((provider) => bandColor(provider, dark)), [dark]);

  const definition = useMemo(() => {
    const labelStep = Math.max(1, Math.floor(series.columns.length / 5));
    const keys = series.columns.map((column) => column.key);
    const labelByKey = new Map(series.columns.map((column) => [column.key, column.label]));
    const formatTick = (value: number) => (metric === "cost" ? formatUsd(value) : formatTokens(value));
    const formatValue = (value: unknown) => {
      const n = typeof value === "number" ? value : Number(value);
      if (!Number.isFinite(n)) return "—";
      return formatTick(n);
    };

    const base = defineChart({
      svgAnimation:
        typeof matchMedia === "undefined" || !matchMedia("(prefers-reduced-motion: reduce)").matches,
      focus: "group-x",
      maxFocusDistance: Number.POSITIVE_INFINITY,
      focusRing: false,
      clip: true,
      marks: [
        areaY(rows, {
          x: "key",
          y: "value",
          z: "provider",
          color: "provider",
          fillOpacity: 0.22,
        }),
        lineY(rows, {
          x: "key",
          y: "value",
          z: "provider",
          color: "provider",
          strokeWidth: 2,
        }),
        crosshair({
          x: {
            stroke: dark ? "rgba(255,255,255,0.28)" : "rgba(0,0,0,0.22)",
            strokeWidth: 1,
          },
          y: false,
        }),
      ],
      x: {
        scale: () => scalePoint<string>().domain(keys).padding(0.08),
        axis: {
          ticks: {
            values: keys.filter((_, index) => index % labelStep === 0 || index === keys.length - 1),
            format: (value) => labelByKey.get(String(value)) ?? String(value),
          },
        },
      },
      y: {
        scale: scaleLinear,
        nice: true,
        grid: true,
        axis: {
          ticks: {
            count: 4,
            format: (value) => formatTick(Number(value)),
          },
        },
      },
      color: {
        domain: [...PROVIDERS],
        range: colors,
      },
      margin: { top: 10, right: 8, bottom: 28, left: 52 },
    });

    return defineChart(base, {
      tooltip: {
        use: tooltip,
        className: "usage-chart-tooltip",
        content: (points) => {
          const key = String(points[0]?.xValue ?? "");
          return {
            title: labelByKey.get(key) ?? key,
            rows: points.map((point) => {
              const provider = String(point.datum.provider) as AgentId;
              return {
                label: providerLabel(provider),
                value: formatValue(point.yValue),
                color: point.color,
              };
            }),
          };
        },
      },
    });
  }, [colors, dark, metric, rows, series.columns]);

  return (
    <section className="usage-chart" aria-label="Usage over time">
      <div className="mb-3 flex items-center gap-3">
        <span className="text-[13px] font-semibold tracking-[-0.01em]">
          {hourly ? "Hourly" : "Daily"} {metric === "cost" ? "cost" : "tokens"}
        </span>
        <div className="flex-1" />
        <div
          className="inline-grid grid-flow-col overflow-hidden rounded-md border border-[var(--hair)]"
          role="group"
          aria-label="Metric"
        >
          {(
            [
              ["cost", "Cost"],
              ["tokens", "Tokens"],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              type="button"
              className={`h-7 cursor-pointer rounded-none border-0 px-2.5 text-[10.5px] font-semibold tracking-[0.05em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] ${
                metric === value
                  ? "bg-[var(--fill)] text-[var(--fill-ink)]"
                  : "bg-transparent text-[var(--mute)]"
              }`}
              aria-pressed={metric === value}
              onClick={() => onMetricChange(value)}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {empty ? (
        <p className="py-14 text-center text-[13px] text-[var(--mute)]">No activity in this window.</p>
      ) : (
        <div className="usage-chart-host">
          <Chart
            definition={definition}
            height={260}
            ariaLabel={metric === "cost" ? "Daily cost by provider" : "Daily tokens by provider"}
          />
        </div>
      )}
    </section>
  );
}
