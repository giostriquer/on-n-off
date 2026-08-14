import { describe, expect, it } from "vitest";
import { buildChartSeries } from "./usageChart";
import type { FoldedUsage } from "./usageMerge";

function emptyFolded(overrides: Partial<FoldedUsage> = {}): FoldedUsage {
  return {
    costUsd: 0,
    cacheSavingsUsd: 0,
    totalTokens: 0,
    records: 0,
    sessions: 0,
    activeDays: 0,
    tokens: {
      uncachedInputTokens: 0,
      cachedInputTokens: 0,
      cacheCreationTokens: 0,
      outputTokens: 0,
      reasoningTokens: 0,
    },
    providers: [],
    models: [],
    daily: [],
    hourly: [],
    ...overrides,
  };
}

describe("buildChartSeries", () => {
  it("sparse Full time only keeps days with activity", () => {
    const folded = emptyFolded({
      daily: [
        {
          day: "2026-01-02",
          costUsd: 1,
          totalTokens: 100,
          byProvider: {
            claude: { costUsd: 1, totalTokens: 100 },
            codex: { costUsd: 0, totalTokens: 0 },
            antigravity: { costUsd: 0, totalTokens: 0 },
          },
        },
        {
          day: "2026-03-10",
          costUsd: 2,
          totalTokens: 200,
          byProvider: {
            claude: { costUsd: 0, totalTokens: 0 },
            codex: { costUsd: 2, totalTokens: 200 },
            antigravity: { costUsd: 0, totalTokens: 0 },
          },
        },
      ],
    });
    const series = buildChartSeries({
      folded,
      metric: "cost",
      sinceDay: "2020-01-01",
      untilDay: "2026-08-13",
      hourly: false,
      sparse: true,
    });
    expect(series.columns.map((c) => c.key)).toEqual(["2026-01-02", "2026-03-10"]);
  });
});
