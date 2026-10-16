import { act, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { foldUsage } from "$lib/usageMerge";
import { UsageChart } from "./UsageChart";

type Definition = { color: { domain: string[]; range: string[] } };

const chart = vi.hoisted(() => vi.fn((_props: { definition: Definition }) => null));
vi.mock("@tanstack/charts/react", () => ({ Chart: (props: { definition: Definition }) => chart(props) }));

const folded = foldUsage({
  readAt: "2026-08-15T12:00:00.000Z",
  timeZone: "UTC",
  sinceDay: "2026-08-15",
  untilDay: "2026-08-15",
  buckets: [
    {
      day: "2026-08-15",
      provider: "codex",
      model: "gpt-5.6-sol",
      totals: { uncachedInputTokens: 100, cachedInputTokens: 0, cacheCreationTokens: 0, outputTokens: 50, reasoningTokens: 0 },
      costUsd: 1,
      cacheSavingsUsd: 0,
      costSource: "modelPriced",
      records: 1,
      unpricedRecords: 0,
      sessions: 1,
    },
  ],
  sources: [],
  pricing: { status: "fresh", source: "fixture", knownModels: 1 },
  scanDurationMs: 1,
  cacheHit: false,
});

function drawnColors() {
  return chart.mock.calls.at(-1)![0].definition.color;
}

afterEach(() => {
  document.documentElement.classList.remove("dark");
  chart.mockClear();
});

describe("UsageChart", () => {
  it("draws a band for every provider Usage reads, in the theme's colours", async () => {
    render(<UsageChart folded={folded} metric="cost" onMetricChange={() => undefined} sinceDay="2026-08-15" untilDay="2026-08-15" />);

    expect(drawnColors()).toEqual({ domain: ["claude", "codex", "muse"], range: ["#d9773a", "#2a2a2a", "#0082fb"] });

    act(() => document.documentElement.classList.add("dark"));
    await waitFor(() =>
      expect(drawnColors()).toEqual({ domain: ["claude", "codex", "muse"], range: ["#d97757", "#ececec", "#0082fb"] }),
    );
  });
});
