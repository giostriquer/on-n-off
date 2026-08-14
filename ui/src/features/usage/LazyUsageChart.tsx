import { lazy, Suspense } from "react";
import type { UsageChartProps } from "./UsageChart";

const UsageChart = lazy(() =>
  import("./UsageChart").then((module) => ({ default: module.UsageChart })),
);

export function LazyUsageChart(props: UsageChartProps) {
  return (
    <Suspense
      fallback={
        <section className="usage-chart min-h-[304px]" aria-label="Usage over time">
          <p className="py-14 text-center text-[13px] text-[var(--mute)]">Loading chart…</p>
        </section>
      }
    >
      <UsageChart {...props} />
    </Suspense>
  );
}
