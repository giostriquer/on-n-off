import type { AgentId } from "./types";
import { PROVIDERS, type FoldedUsage, type PeriodTotals } from "./usageMerge";
import type { UsageMetric } from "./usageTypes";
import { enumerateDays, enumerateHourStarts, formatDayShort } from "./usageFormat";

export type ChartBand = {
  provider: AgentId;
  value: number;
};

export type ChartColumn = {
  key: string;
  label: string;
  total: number;
  bands: ChartBand[];
};

export type ChartSeries = {
  columns: ChartColumn[];
  max: number;
};

function periodValue(period: PeriodTotals | undefined, provider: AgentId, metric: UsageMetric): number {
  const slice = period?.byProvider[provider];
  if (!slice) return 0;
  return metric === "cost" ? slice.costUsd : slice.totalTokens;
}

/** Build stacked columns for the Usage chart (daily or hourly). */
export function buildChartSeries(args: {
  folded: FoldedUsage;
  metric: UsageMetric;
  sinceDay: string;
  untilDay: string;
  sinceTime?: string;
  untilTime?: string;
  hourly: boolean;
}): ChartSeries {
  const { folded, metric, hourly } = args;
  const byKey = new Map<string, PeriodTotals>();
  if (hourly) {
    for (const period of folded.hourly) {
      if (period.hourStart) byKey.set(period.hourStart, period);
    }
  } else {
    for (const period of folded.daily) {
      byKey.set(period.day, period);
    }
  }

  const keys = hourly
    ? args.sinceTime && args.untilTime
      ? [...enumerateHourStarts(args.sinceTime, args.untilTime)]
      : folded.hourly.map((p) => p.hourStart!).filter(Boolean)
    : [...enumerateDays(args.sinceDay, args.untilDay)];

  const columns: ChartColumn[] = keys.map((key) => {
    const period = byKey.get(key);
    const bands = PROVIDERS.map((provider) => ({
      provider,
      value: periodValue(period, provider, metric),
    }));
    const total = bands.reduce((sum, band) => sum + band.value, 0);
    const label = hourly
      ? new Date(key).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric" })
      : formatDayShort(key);
    return { key, label, total, bands };
  });

  const max = Math.max(0, ...columns.map((c) => c.total));
  return { columns, max: max === 0 ? 1 : max };
}

export type ChartRow = {
  key: string;
  label: string;
  provider: AgentId;
  value: number;
};

/** Flatten stacked columns into long rows for TanStack Charts. */
export function toChartRows(series: ChartSeries): ChartRow[] {
  const rows: ChartRow[] = [];
  for (const column of series.columns) {
    for (const band of column.bands) {
      rows.push({
        key: column.key,
        label: column.label,
        provider: band.provider,
        value: band.value,
      });
    }
  }
  return rows;
}
