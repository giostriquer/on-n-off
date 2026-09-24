import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import type { UsageHistoryStatus } from "$lib/usageTypes";
import { UsageHistoryCard } from "./UsageHistoryCard";

const calls = vi.hoisted(() => ({ usageHistoryStatus: vi.fn(), clearUsageHistory: vi.fn() }));
vi.mock("$lib/api", () => ({
  usageHistoryStatus: calls.usageHistoryStatus,
  clearUsageHistory: calls.clearUsageHistory,
}));

const KEPT: UsageHistoryStatus = {
  state: "kept",
  keptSince: "2026-08-07T12:00:00.000Z",
  foldedThrough: "2026-09-17T00:00:00.000Z",
  bytes: 184_320,
};
const EMPTY: UsageHistoryStatus = { state: "empty", bytes: 0 };

beforeEach(() => {
  calls.usageHistoryStatus.mockReset();
  calls.clearUsageHistory.mockReset();
  calls.usageHistoryStatus.mockResolvedValue(KEPT);
  calls.clearUsageHistory.mockResolvedValue(EMPTY);
});

function renderCard() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <UsageHistoryCard />
    </QueryClientProvider>,
  );
  return { invalidate };
}

it("says how far back usage is kept and how much room it takes", async () => {
  renderCard();

  const card = await screen.findByRole("region", { name: "Usage history" });
  await waitFor(() => expect(card).toHaveTextContent("Kept since Aug 7, 2026"));
  expect(card).toHaveTextContent("180 KB");
});

it("offers nothing to clear before any usage is kept", async () => {
  calls.usageHistoryStatus.mockResolvedValue(EMPTY);
  renderCard();

  const card = await screen.findByRole("region", { name: "Usage history" });
  await waitFor(() => expect(card).toHaveTextContent("Nothing kept yet"));
  expect(screen.queryByRole("button", { name: "Clear history" })).toBeNull();
});

it("clears only once the loss is confirmed, then shows what is left and rereads usage", async () => {
  const user = userEvent.setup();
  const { invalidate } = renderCard();

  await user.click(await screen.findByRole("button", { name: "Clear history" }));
  expect(calls.clearUsageHistory).not.toHaveBeenCalled();
  expect(screen.getByRole("group", { name: "Confirm clearing usage history" })).toHaveTextContent(
    "lost for good",
  );
  await user.click(screen.getByRole("button", { name: "Confirm clear" }));

  expect(calls.clearUsageHistory).toHaveBeenCalledTimes(1);
  const card = screen.getByRole("region", { name: "Usage history" });
  await waitFor(() => expect(card).toHaveTextContent("Nothing kept yet"));
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ["usage"] });
});

it("keeps the history when the confirmation is cancelled", async () => {
  const user = userEvent.setup();
  renderCard();

  await user.click(await screen.findByRole("button", { name: "Clear history" }));
  await user.click(screen.getByRole("button", { name: "Cancel" }));

  expect(calls.clearUsageHistory).not.toHaveBeenCalled();
  expect(screen.queryByRole("group", { name: "Confirm clearing usage history" })).toBeNull();
  expect(screen.getByRole("region", { name: "Usage history" })).toHaveTextContent("Kept since Aug 7, 2026");
});

it("says a history file that does not read is left alone and still lets it be cleared", async () => {
  calls.usageHistoryStatus.mockResolvedValue({ state: "unreadable", bytes: 12 });
  renderCard();

  const card = await screen.findByRole("region", { name: "Usage history" });
  await waitFor(() => expect(card).toHaveTextContent("could not be read"));
  expect(screen.getByRole("button", { name: "Clear history" })).toBeEnabled();
});

it("shows why clearing failed", async () => {
  const user = userEvent.setup();
  calls.clearUsageHistory.mockRejectedValue({ kind: "message", message: "Could not clear the usage history: denied" });
  renderCard();

  await user.click(await screen.findByRole("button", { name: "Clear history" }));
  await user.click(screen.getByRole("button", { name: "Confirm clear" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("Could not clear the usage history: denied");
});
