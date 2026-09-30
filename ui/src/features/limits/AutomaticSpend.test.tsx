import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatClock } from "$lib/limitsFormat";
import { AutomaticSpend } from "./AutomaticSpend";

// The banked reset an automatic alert is waiting to spend, as its account's card shows it.

const pendingResetSpends = vi.hoisted(() => vi.fn());
const cancelResetSpend = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  pendingResetSpends,
  cancelResetSpend,
  onSharedReadChanged: () => Promise.resolve(() => {}),
}));

const NOW = Date.parse("2026-10-01T12:00:00Z");
const DUE = "2026-10-01T12:08:00Z";

function renderSpend(accountId = "acct-work") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><AutomaticSpend accountId={accountId} now={NOW} /></QueryClientProvider>);
}

beforeEach(() => {
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

  it("shows nothing on another account's card", async () => {
    renderSpend("acct-other");

    await waitFor(() => expect(pendingResetSpends).toHaveBeenCalled());
    expect(screen.queryByText(/Using a banked reset/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Cancel the automatic banked reset" })).toBeNull();
  });
});
