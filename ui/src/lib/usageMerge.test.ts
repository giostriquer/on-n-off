import { describe, expect, it } from "vitest";
import { formatTokens, formatUsd, makeWindow } from "./usageFormat";
import { foldUsage, PROVIDERS, providerLabel } from "./usageMerge";
import { buildChartSeries, toChartRows } from "./usageChart";
import type { UsageSummary } from "./usageTypes";

function summary(overrides: Partial<UsageSummary> = {}): UsageSummary {
  return {
    readAt: "2026-08-13T00:00:00.000Z",
    timeZone: "UTC",
    sinceDay: "2026-08-01",
    untilDay: "2026-08-31",
    buckets: [
      {
        day: "2026-08-07",
        provider: "claude",
        model: "claude-fable-5",
        totals: {
          uncachedInputTokens: 100,
          cachedInputTokens: 0,
          cacheCreationTokens: 0,
          outputTokens: 50,
          reasoningTokens: 0,
        },
        costUsd: 1.25,
        cacheSavingsUsd: 0,
        costSource: "modelPriced",
        records: 1,
        unpricedRecords: 0,
        sessions: 1,
      },
      {
        day: "2026-08-07",
        provider: "codex",
        model: "gpt-5.6",
        totals: {
          uncachedInputTokens: 200,
          cachedInputTokens: 0,
          cacheCreationTokens: 0,
          outputTokens: 10,
          reasoningTokens: 0,
        },
        costUsd: 0.5,
        cacheSavingsUsd: 0,
        costSource: "modelPriced",
        records: 2,
        unpricedRecords: 0,
        sessions: 1,
      },
      {
        day: "2026-08-08",
        provider: "claude",
        model: "claude-fable-5",
        totals: {
          uncachedInputTokens: 10,
          cachedInputTokens: 0,
          cacheCreationTokens: 0,
          outputTokens: 5,
          reasoningTokens: 0,
        },
        costUsd: 0.1,
        cacheSavingsUsd: 0,
        costSource: "modelPriced",
        records: 1,
        unpricedRecords: 0,
        sessions: 1,
      },
    ],
    sources: [
      {
        provider: "claude",
        status: "ok",
        scannedFiles: 1,
        skippedFiles: 0,
        malformedRecords: 0,
        distinctSessions: 1,
        resolvedPath: "/claude",
      },
      {
        provider: "codex",
        status: "missing",
        scannedFiles: 0,
        skippedFiles: 0,
        malformedRecords: 0,
        distinctSessions: 0,
        message: "No transcript directory on this environment.",
        resolvedPath: "/codex",
      },
    ],
    pricing: {
      status: "fresh",
      source: "litellm",
      knownModels: 10,
    },
    scanDurationMs: 12,
    ...overrides,
  };
}

describe("foldUsage", () => {
  it("sums cost and tokens and ranks models", () => {
    const folded = foldUsage(summary());
    expect(folded.costUsd).toBeCloseTo(1.85);
    expect(folded.totalTokens).toBe(375);
    expect(folded.sessions).toBe(1);
    expect(folded.models[0]?.model).toBe("claude-fable-5");
    expect(folded.providers.map((p) => p.provider)).toEqual(["claude", "codex"]);
    expect(folded.daily[0]?.byProvider.claude.costUsd).toBeCloseTo(1.25);
    expect(folded.daily[0]?.byProvider.codex.costUsd).toBeCloseTo(0.5);
  });

  it("handles empty and null", () => {
    expect(foldUsage(null).totalTokens).toBe(0);
    expect(foldUsage(summary({ buckets: [] })).sessions).toBe(1);
  });

  it("labels providers for display", () => {
    expect(providerLabel("claude")).toBe("Claude");
    expect(providerLabel("codex")).toBe("Codex");
  });

  it("aggregates token breakdown and active days", () => {
    const folded = foldUsage(summary());
    expect(folded.tokens.uncachedInputTokens).toBe(310);
    expect(folded.tokens.outputTokens).toBe(65);
    expect(folded.activeDays).toBe(2);
  });
});

describe("buildChartSeries", () => {
  it("stacks providers for enumerated days", () => {
    const folded = foldUsage(summary());
    const series = buildChartSeries({
      folded,
      metric: "cost",
      sinceDay: "2026-08-07",
      untilDay: "2026-08-08",
      hourly: false,
    });
    expect(series.columns).toHaveLength(2);
    expect(series.columns[0]?.bands[0]?.provider).toBe("claude");
    expect(series.columns[0]?.total).toBeCloseTo(1.75);
    expect(series.max).toBeGreaterThan(0);
  });

  it("returns unit max when all zeros", () => {
    const folded = foldUsage(summary({ buckets: [] }));
    const series = buildChartSeries({
      folded,
      metric: "tokens",
      sinceDay: "2026-08-07",
      untilDay: "2026-08-07",
      hourly: false,
    });
    expect(series.max).toBe(1);
  });

  it("flattens stacked columns into chart rows", () => {
    const series = buildChartSeries({
      folded: foldUsage(summary()),
      metric: "cost",
      sinceDay: "2026-08-07",
      untilDay: "2026-08-07",
      hourly: false,
    });
    const rows = toChartRows(series);
    expect(rows.length).toBe(series.columns.length * PROVIDERS.length);
    expect(rows.some((row) => row.provider === "claude" && row.value > 0)).toBe(true);
  });
});

describe("usageFormat", () => {
  it("formats money and compact tokens", () => {
    expect(formatUsd(1.5)).toBe("$1.50");
    expect(formatTokens(19_900)).toMatch(/K$/);
  });

  it("makeWindow uses hour resolution for 1 day", () => {
    const w = makeWindow(1, new Date("2026-08-13T12:00:00.000Z"));
    expect(w.resolution).toBe("hour");
    expect(w.sinceTime).toBeTruthy();
    expect(w.untilTime).toBeTruthy();
  });

  it("makeWindow uses day resolution for 30 days", () => {
    const w = makeWindow(30, new Date("2026-08-13T12:00:00.000Z"));
    expect(w.resolution).toBe("day");
    expect(w.sinceTime).toBeUndefined();
  });

  it("makeWindow Full time starts at the usage epoch", () => {
    const w = makeWindow(0, new Date("2026-08-13T12:00:00.000Z"));
    expect(w.fullTime).toBe(true);
    expect(w.sinceDay).toBe("2020-01-01");
    expect(w.untilDay).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(w.resolution).toBe("day");
  });
});
