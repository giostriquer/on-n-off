import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UsageSummary } from "$lib/usageTypes";
import { OverviewUsageCard } from "./OverviewUsageCard";

const usageSummary = vi.hoisted(() => vi.fn(() => new Promise(() => undefined)));
const preloadUsageChart = vi.hoisted(() => vi.fn(() => Promise.resolve()));
const renderUsageChart = vi.hoisted(() => vi.fn((_props: unknown) => null));

vi.mock("$lib/api", () => ({ usageSummary }));
vi.mock("@tanstack/react-router", () => ({
  Link: ({ children, to, ...props }: React.ComponentProps<"a"> & { to: string }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
}));
vi.mock("./LazyUsageChart", () => ({
  LazyUsageChart: (props: unknown) => renderUsageChart(props),
  preloadUsageChart,
}));

beforeEach(() => {
  usageSummary.mockReset();
  usageSummary.mockImplementation(() => new Promise(() => undefined));
  preloadUsageChart.mockClear();
  renderUsageChart.mockClear();
});

const missingSummary: UsageSummary = {
  readAt: "2026-08-15T12:00:00.000Z",
  timeZone: "America/Sao_Paulo",
  sinceDay: "2026-07-17",
  untilDay: "2026-08-15",
  buckets: [],
  sources: [
    {
      provider: "codex",
      status: "missing",
      scannedFiles: 0,
      skippedFiles: 0,
      malformedRecords: 0,
      distinctSessions: 0,
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
  pricing: { status: "fresh", source: "fixture", knownModels: 0 },
  scanDurationMs: 5,
  cacheHit: false,
};

const availableSummary: UsageSummary = {
  ...missingSummary,
  sources: missingSummary.sources.map((source) => ({ ...source, status: "ok" })),
};

describe("OverviewUsageCard", () => {
  it("waits for the selected provider before scanning transcripts", async () => {
    usageSummary.mockClear();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={client}>
        <OverviewUsageCard ready={false} />
      </QueryClientProvider>,
    );

    await Promise.resolve();
    expect(usageSummary).not.toHaveBeenCalled();

    view.rerender(
      <QueryClientProvider client={client}>
        <OverviewUsageCard ready />
      </QueryClientProvider>,
    );
    await waitFor(() => expect(usageSummary).toHaveBeenCalledTimes(1));
  });

  it("preloads the chart when the Usage link receives intent", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <OverviewUsageCard ready={false} />
      </QueryClientProvider>,
    );

    const link = screen.getByRole("link", { name: "Open usage" });
    fireEvent.mouseEnter(link);
    fireEvent.focus(link);
    expect(preloadUsageChart).toHaveBeenCalledTimes(2);
  });

  it("preloads the chart from the missing-transcripts Usage link", async () => {
    usageSummary.mockResolvedValue(missingSummary);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <OverviewUsageCard />
      </QueryClientProvider>,
    );

    await waitFor(() => expect(screen.getAllByRole("link", { name: "Open usage" })).toHaveLength(2));
    const links = screen.getAllByRole("link", { name: "Open usage" });
    fireEvent.mouseEnter(links[1]!);
    fireEvent.focus(links[1]!);
    expect(preloadUsageChart).toHaveBeenCalledTimes(2);
  });

  it("does not rerender the settled chart when only the provider parent changes", async () => {
    usageSummary.mockResolvedValue(availableSummary);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const card = (agent: string) => (
      <QueryClientProvider client={client}>
        <div data-agent={agent}>
          <OverviewUsageCard />
        </div>
      </QueryClientProvider>
    );
    const view = render(card("codex"));

    await waitFor(() => expect(renderUsageChart).toHaveBeenCalledTimes(1));
    view.rerender(card("claude"));

    expect(renderUsageChart).toHaveBeenCalledTimes(1);
  });
});
