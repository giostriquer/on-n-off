import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatClock } from "$lib/limitsFormat";
import { AutomaticSpend } from "./AutomaticSpend";

const pendingResetSpends = vi.hoisted(() => vi.fn());
const cancelResetSpend = vi.hoisted(() => vi.fn());

const listeners = vi.hoisted(() => new Set<(change: { source: string }) => void>());

vi.mock("$lib/api", () => ({
  pendingResetSpends,
  cancelResetSpend,
  onSharedReadChanged: (listener: (change: { source: string }) => void) => {
    listeners.add(listener);
    return Promise.resolve(() => listeners.delete(listener));
  },
}));

const NOW = Date.parse("2026-10-01T12:00:00Z");
const DUE = "2026-10-01T12:08:00Z";

function renderSpend(...accountIds: string[]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      {(accountIds.length ? accountIds : ["acct-work"]).map((accountId) => <AutomaticSpend key={accountId} accountId={accountId} now={NOW} />)}
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  listeners.clear();
  pendingResetSpends.mockReset().mockResolvedValue([{ accountId: "acct-work", dueAt: DUE }]);
  cancelResetSpend.mockReset().mockResolvedValue(true);
});

describe("a banked reset waiting to be used by itself", () => {
  it("says when it will be used, and a Cancel keeps it", async () => {
    renderSpend();

    expect(await screen.findByText(`Using a banked reset at ${formatClock(DUE)} · in 8m`)).toBeTruthy();
    pendingResetSpends.mockResolvedValue([]);
    fireEvent.click(screen.getByRole("button", { name: "Cancel the automatic banked reset" }));

    await waitFor(() => expect(cancelResetSpend).toHaveBeenCalledWith("acct-work"));
    await waitFor(() => expect(screen.queryByText(/Using a banked reset/)).toBeNull());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says so when the reset was no longer waiting to be cancelled", async () => {
    cancelResetSpend.mockResolvedValue(false);
    renderSpend();

    fireEvent.click(await screen.findByRole("button", { name: "Cancel the automatic banked reset" }));

    expect((await screen.findByRole("alert")).textContent).toBe("It was already being used, or no longer waiting.");
  });

  it("shows only on the card of the account it is for", async () => {
    renderSpend("acct-work", "acct-other");

    await screen.findByText(/Using a banked reset/);
    expect(screen.getAllByText(/Using a banked reset/)).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "Cancel the automatic banked reset" })).toHaveLength(1);
  });

  it("says why a cancel failed and lets it be tried again", async () => {
    cancelResetSpend.mockRejectedValueOnce(new Error("The monitor is busy."));
    renderSpend();

    const cancel = await screen.findByRole("button", { name: "Cancel the automatic banked reset" });
    fireEvent.click(cancel);

    expect((await screen.findByRole("alert")).textContent).toBe("The monitor is busy.");
    expect(cancel).toBeEnabled();
    pendingResetSpends.mockResolvedValue([]);
    fireEvent.click(cancel);
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(cancelResetSpend).toHaveBeenCalledTimes(2);
  });

  it("appears on a card already open once the monitor says a reset is waiting", async () => {
    pendingResetSpends.mockResolvedValueOnce([]);
    renderSpend();
    await waitFor(() => expect(listeners.size).toBe(1));
    expect(screen.queryByText(/Using a banked reset/)).toBeNull();

    for (const listener of listeners) listener({ source: "limits:reset-spends" });

    expect(await screen.findByText(/Using a banked reset/)).toBeTruthy();
  });
});
