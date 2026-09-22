import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UsageSummary } from "$lib/usageTypes";
import { Usage } from "./Usage";

const usageSummary = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({ usageSummary }));
vi.mock("./LazyUsageChart", () => ({
  LazyUsageChart: ({
    sinceDay,
    untilDay,
    sinceTime,
    untilTime,
  }: {
    sinceDay: string;
    untilDay: string;
    sinceTime?: string;
    untilTime?: string;
  }) => (
    <section
      aria-label={`Usage chart ${sinceDay} to ${untilDay}`}
      data-since-time={sinceTime}
      data-until-time={untilTime}
    >
      Chart
    </section>
  ),
  preloadUsageChart: vi.fn(() => Promise.resolve()),
}));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function summary({
  sinceDay = "2026-07-17",
  untilDay = "2026-08-15",
  costUsd = 12.5,
}: {
  sinceDay?: string;
  untilDay?: string;
  costUsd?: number;
} = {}): UsageSummary {
  return {
    readAt: "2026-08-15T12:00:00.000Z",
    timeZone: "America/Sao_Paulo",
    sinceDay,
    untilDay,
    buckets: [
      {
        day: sinceDay,
        provider: "codex",
        model: "gpt-5.6-sol",
        totals: {
          uncachedInputTokens: 100,
          cachedInputTokens: 200,
          cacheCreationTokens: 10,
          outputTokens: 20,
          reasoningTokens: 5,
        },
        costUsd,
        cacheSavingsUsd: 25,
        costSource: "modelPriced",
        records: 2,
        unpricedRecords: 0,
        sessions: 1,
      },
    ],
    sources: [
      {
        provider: "codex",
        status: "ok",
        scannedFiles: 1,
        skippedFiles: 0,
        malformedRecords: 0,
        distinctSessions: 1,
        resolvedPath: "C:/fixture/.codex/sessions",
      },
      {
        provider: "claude",
        status: "missing",
        scannedFiles: 0,
        skippedFiles: 0,
        malformedRecords: 0,
        distinctSessions: 0,
        resolvedPath: "C:/fixture/.claude/projects",
      },
    ],
    pricing: {
      status: "fresh",
      source: "fixture",
      knownModels: 1,
    },
    scanDurationMs: 25,
    cacheHit: false,
  };
}

function renderUsage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Number.POSITIVE_INFINITY } },
  });
  return render(
    <QueryClientProvider client={client}>
      <Usage />
    </QueryClientProvider>,
  );
}

function expectTotal(value: string) {
  expect(within(screen.getByRole("region", { name: "Usage totals" })).getAllByText(value).length).toBeGreaterThan(0);
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-08-15T15:00:00.000Z"));
  usageSummary.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Usage loading continuity", () => {
  it("keeps the displayed totals and chart bounds while a different window loads", async () => {
    const first = deferred<UsageSummary>();
    const next = deferred<UsageSummary>();
    usageSummary.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => next.promise);
    renderUsage();

    await act(async () => first.resolve(summary()));
    await screen.findByRole("region", { name: "Usage totals" });
    expectTotal("$12.50");
    expect(screen.getByRole("region", { name: "Usage chart 2026-07-17 to 2026-08-15" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "90 days" }));

    expectTotal("$12.50");
    expect(screen.getByRole("region", { name: "Usage chart 2026-07-17 to 2026-08-15" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Updating to May 18 to Aug 15");
    expect(screen.getByTestId("usage-screen")).toHaveAttribute("aria-busy", "true");

    await act(async () =>
      next.resolve(summary({ sinceDay: "2026-05-18", untilDay: "2026-08-15", costUsd: 99 })),
    );
    await waitFor(() => expectTotal("$99.00"));
    expect(screen.getByRole("region", { name: "Usage chart 2026-05-18 to 2026-08-15" })).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("keeps the current body during a forced refresh", async () => {
    const first = deferred<UsageSummary>();
    const refreshed = deferred<UsageSummary>();
    usageSummary.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => refreshed.promise);
    renderUsage();

    await act(async () => first.resolve(summary()));
    await screen.findByRole("region", { name: "Usage totals" });
    expectTotal("$12.50");

    fireEvent.click(screen.getByRole("button", { name: "Refresh usage" }));

    expectTotal("$12.50");
    expect(screen.getByRole("region", { name: "Token metrics" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Usage chart 2026-07-17 to 2026-08-15" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Refreshing Jul 17 to Aug 15");
    await waitFor(() => expect(usageSummary.mock.calls[1]?.[0]).toMatchObject({ force: true }));

    await act(async () => refreshed.resolve(summary({ costUsd: 13.5 })));
    await waitFor(() => expectTotal("$13.50"));
  });

  it("keeps the last complete body and shows an error when refresh fails", async () => {
    const first = deferred<UsageSummary>();
    const refreshed = deferred<UsageSummary>();
    usageSummary.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => refreshed.promise);
    renderUsage();

    await act(async () => first.resolve(summary()));
    await screen.findByRole("region", { name: "Usage totals" });
    expectTotal("$12.50");
    fireEvent.click(screen.getByRole("button", { name: "Refresh usage" }));
    await act(async () => refreshed.reject(new Error("fixture refresh failed")));

    expect(await screen.findByText(/fixture refresh failed/i)).toBeInTheDocument();
    expectTotal("$12.50");
    expect(screen.getByRole("region", { name: "Usage chart 2026-07-17 to 2026-08-15" })).toBeInTheDocument();
  });

  it("keeps the exact hourly request bounds when a cached window is revisited after the minute changes", async () => {
    vi.setSystemTime(new Date("2026-08-15T15:00:59.900Z"));
    const secondHourly = deferred<UsageSummary>();
    let hourlyCalls = 0;
    usageSummary.mockImplementation((input: { resolution: "day" | "hour" }) => {
      if (input.resolution === "hour") {
        hourlyCalls += 1;
        return hourlyCalls === 1
          ? Promise.resolve(summary({ sinceDay: "2026-08-14", untilDay: "2026-08-15" }))
          : secondHourly.promise;
      }
      return Promise.resolve(summary());
    });
    renderUsage();

    await screen.findByRole("region", { name: "Usage totals" });
    fireEvent.click(screen.getByRole("button", { name: "Past 24h" }));
    const firstHourlyChart = await screen.findByRole("region", {
      name: "Usage chart 2026-08-14 to 2026-08-15",
    });
    expect(firstHourlyChart).toHaveAttribute("data-since-time", "2026-08-14T15:00:00.000Z");
    expect(firstHourlyChart).toHaveAttribute("data-until-time", "2026-08-15T15:00:00.000Z");

    vi.setSystemTime(new Date("2026-08-15T15:01:00.100Z"));
    fireEvent.click(screen.getByRole("button", { name: "30 days" }));
    await screen.findByRole("region", { name: "Usage chart 2026-07-17 to 2026-08-15" });
    fireEvent.click(screen.getByRole("button", { name: "Past 24h" }));

    const revisitedChart = screen.getByRole("region", {
      name: "Usage chart 2026-08-14 to 2026-08-15",
    });
    expect(revisitedChart).toHaveAttribute("data-since-time", "2026-08-14T15:00:00.000Z");
    expect(revisitedChart).toHaveAttribute("data-until-time", "2026-08-15T15:00:00.000Z");
  });
});

function twoDaySummary(): UsageSummary {
  const bucket = (day: string, provider: "claude" | "codex", model: string, costUsd: number) => ({
    day,
    provider,
    model,
    totals: {
      uncachedInputTokens: 100,
      cachedInputTokens: 0,
      cacheCreationTokens: 0,
      outputTokens: 20,
      reasoningTokens: 0,
    },
    costUsd,
    cacheSavingsUsd: 0,
    costSource: "modelPriced" as const,
    records: 1,
    unpricedRecords: 0,
    sessions: 1,
  });
  return {
    ...summary(),
    sinceDay: "2026-08-14",
    untilDay: "2026-08-15",
    buckets: [
      bucket("2026-08-14", "codex", "gpt-5.6-sol", 1),
      bucket("2026-08-15", "codex", "gpt-5.6-sol", 3),
      bucket("2026-08-15", "claude", "claude-fable-5", 1),
    ],
  };
}

describe("Usage day breakdown", () => {
  it("lists days newest first and opens one into its models", async () => {
    usageSummary.mockResolvedValue(twoDaySummary());
    renderUsage();
    await screen.findByRole("region", { name: "Usage totals" });

    fireEvent.click(screen.getByRole("button", { name: "Day" }));
    const rows = screen.getAllByRole("button", { expanded: false }).filter((row) => /Aug 1[45]/.test(row.textContent ?? ""));
    expect(rows.map((row) => row.textContent?.slice(0, 6))).toEqual(["Aug 15", "Aug 14"]);

    fireEvent.click(rows[0]);

    const open = screen.getByRole("button", { expanded: true });
    const models = document.getElementById(open.getAttribute("aria-controls") ?? "");
    expect(models).not.toBeNull();
    expect(within(models as HTMLElement).getByText("gpt-5.6-sol")).toBeTruthy();
    expect(within(models as HTMLElement).getByText("claude-fable-5")).toBeTruthy();
    // One denominator for the column: $3 and $1 of the window's $5, so the rows under a day
    // add up to the day row itself (80.0%).
    expect(rows[0].textContent).toContain("80.0%");
    expect(within(models as HTMLElement).getByText("60.0%")).toBeTruthy();
    expect(within(models as HTMLElement).getByText("20.0%")).toBeTruthy();
  });
});

describe("Usage prices", () => {
  it("shows a model without a public price as unpriced rather than as free", async () => {
    const base = twoDaySummary();
    usageSummary.mockResolvedValue({
      ...base,
      buckets: [
        ...base.buckets,
        {
          ...base.buckets[0],
          day: "2026-08-15",
          model: "codex-auto-review",
          costUsd: 0,
          costSource: "unpriced" as const,
          records: 3,
          unpricedRecords: 3,
        },
      ],
    });
    renderUsage();
    const totals = await screen.findByRole("region", { name: "Usage totals" });

    expect(within(totals).getByText("if billed at full API rate · 1 model has no price yet")).toBeTruthy();
    const modelRow = (name: string, scope: HTMLElement = document.body) =>
      within(scope).getByText(name).closest("div")?.parentElement as HTMLElement;
    const row = modelRow("codex-auto-review");
    expect(within(row).getByText("unpriced")).toBeTruthy();
    expect(within(row).queryByText("$0.00")).toBeNull();
    const priced = modelRow("gpt-5.6-sol");
    expect(within(priced).getByText("$4.00")).toBeTruthy();
    expect(within(priced).queryByText("unpriced")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Day" }));
    fireEvent.click(screen.getAllByRole("button", { expanded: false }).find((day) => /Aug 15/.test(day.textContent ?? "")) as HTMLElement);
    const models = document.getElementById(screen.getByRole("button", { expanded: true }).getAttribute("aria-controls") ?? "") as HTMLElement;
    expect(within(modelRow("codex-auto-review", models)).getByText("unpriced")).toBeTruthy();
    const pricedDayRow = modelRow("gpt-5.6-sol", models);
    expect(within(pricedDayRow).getByText("$3.00")).toBeTruthy();
    expect(within(pricedDayRow).queryByText("unpriced")).toBeNull();
  });

  it("shows a provider whose every model lacks a price as unpriced", async () => {
    const base = twoDaySummary();
    const claudeOnly = base.buckets.filter((bucket) => bucket.provider === "claude");
    usageSummary.mockResolvedValue({
      ...base,
      buckets: [
        ...claudeOnly,
        { ...base.buckets[0], model: "codex-auto-review", costUsd: 0, costSource: "unpriced" as const, records: 2, unpricedRecords: 2 },
      ],
    });
    renderUsage();
    const totals = await screen.findByRole("region", { name: "Usage totals" });

    const providerRow = (label: string) =>
      within(totals)
        .getAllByText(label)
        .find((element) => element.tagName === "SPAN")?.parentElement as HTMLElement;
    expect(within(providerRow("Codex")).getByText("unpriced")).toBeTruthy();
    expect(within(providerRow("Codex")).queryByText("$0.00")).toBeNull();
    expect(within(providerRow("Claude")).getByText("$1.00")).toBeTruthy();
  });
});
