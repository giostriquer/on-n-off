import { describe, expect, it, vi } from "vitest";
import { preloadUsageChart } from "./LazyUsageChart";

const moduleState = vi.hoisted(() => ({ loads: 0 }));

vi.mock("./UsageChart", () => {
  moduleState.loads += 1;
  return { UsageChart: () => null };
});

describe("Usage chart preload", () => {
  it("loads the chart lazily through one shared promise", async () => {
    expect(moduleState.loads).toBe(0);
    const first = preloadUsageChart();
    const second = preloadUsageChart();
    expect(second).toBe(first);
    await first;
    expect(moduleState.loads).toBe(1);
  });
});
