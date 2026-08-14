import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OverviewUsageCard } from "./OverviewUsageCard";

const usageSummary = vi.hoisted(() => vi.fn(() => new Promise(() => undefined)));

vi.mock("$lib/api", () => ({ usageSummary }));
vi.mock("@tanstack/react-router", () => ({
  Link: ({ children, ...props }: React.ComponentProps<"a">) => <a {...props}>{children}</a>,
}));
vi.mock("./UsageChart", () => ({ UsageChart: () => null }));

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
});
