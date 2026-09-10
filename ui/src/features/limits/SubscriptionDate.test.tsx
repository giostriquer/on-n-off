import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { SubscriptionDate } from "./SubscriptionDate";
import type { SubscriptionReading } from "$lib/subscriptionTypes";
afterEach(cleanup);
function reading(kind: "renews" | "expires" | "paidThrough", stale = false): SubscriptionReading {
  return { connected: false, unavailable: false, metadata: { date: "2026-10-10T12:00:00Z", kind, source: kind === "paidThrough" ? "localToken" : "billing", checkedAt: "2026-09-10T12:00:00Z", stale } };
}
it("distinguishes cancellation from renewal", () => {
  const { rerender } = render(<SubscriptionDate reading={reading("expires")} />);
  expect(screen.getByText(/Expires on/)).toHaveTextContent("2026");
  expect(screen.queryByText(/Renews on/)).not.toBeInTheDocument();
  rerender(<SubscriptionDate reading={reading("renews")} />);
  expect(screen.getByText(/Renews on/)).toBeInTheDocument();
});
it("labels token fallback as cached without implying cancellation", () => {
  render(<SubscriptionDate reading={reading("paidThrough", true)} />);
  expect(screen.getByText(/Paid through/)).toHaveTextContent(/cached/);
  expect(screen.getByText(/Last checked/)).not.toBeVisible();
  fireEvent.click(screen.getByText(/Paid through/));
  expect(screen.getByText(/Last checked/)).toBeVisible();
  expect(screen.queryByText(/Expires on|Renews on/)).not.toBeInTheDocument();
});
it("marks retained billing as stale after failure", () => {
  render(<SubscriptionDate reading={{ ...reading("expires", true), unavailable: true }} />);
  expect(screen.getByText(/Expires on/)).toHaveTextContent(/stale/);
  expect(screen.getByText(/Billing could not be checked/)).not.toBeVisible();
  fireEvent.click(screen.getByText(/Expires on/));
  expect(screen.getByText(/Billing could not be checked/)).toBeVisible();
});
it("does not invent a date for an empty billing observation", () => {
  const value = reading("renews"); value.metadata!.date = null; value.metadata!.kind = null;
  render(<SubscriptionDate reading={value} />);
  expect(screen.getByText(/Subscription date unavailable/)).toBeInTheDocument();
});

// IPC is the boundary: a normal render must not access browser cookies.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { waitFor } from "@testing-library/react";
import { vi } from "vitest";
import * as api from "$lib/api";
import { CodexSubscription } from "./SubscriptionDate";
vi.mock("$lib/api", () => ({readCodexSubscription:vi.fn(),connectCodexBilling:vi.fn()}));
vi.mock("$lib/useSharedRead", () => ({useSharedRead:vi.fn()}));
it("imports a browser session only after an explicit click for that account", async () => {
  vi.mocked(api.readCodexSubscription).mockResolvedValue({...reading("paidThrough"),browserSupported:true});
  vi.mocked(api.connectCodexBilling).mockClear().mockResolvedValue();
  const client = new QueryClient({defaultOptions:{queries:{retry:false}}});
  render(<QueryClientProvider client={client}><CodexSubscription accountId="account-a" current /></QueryClientProvider>);
  await screen.findByText(/Paid through/);
  expect(api.connectCodexBilling).not.toHaveBeenCalled();
  expect(screen.getByText("Use browser session")).not.toBeVisible();
  fireEvent.click(screen.getByText(/Paid through/));
  fireEvent.click(screen.getByRole("button",{name:"Use browser session"}));
  await waitFor(() => expect(api.connectCodexBilling).toHaveBeenCalledWith("account-a"));
  await waitFor(() => expect(screen.getByRole("button",{name:"Use browser session"})).not.toBeDisabled());
});
it("does not offer browser import on an unsupported platform", async () => {
  vi.mocked(api.readCodexSubscription).mockResolvedValue({...reading("paidThrough"),browserSupported:false});
  const client = new QueryClient({defaultOptions:{queries:{retry:false}}});
  render(<QueryClientProvider client={client}><CodexSubscription accountId="account-a" current /></QueryClientProvider>);
  fireEvent.click(await screen.findByText(/Paid through/));
  expect(screen.queryByRole("button",{name:"Use browser session"})).not.toBeInTheDocument();
  expect(screen.getByText(/macOS only/)).toBeVisible();
});
it("does not reread saved metadata every minute and still responds to invalidation", async () => {
  vi.mocked(api.readCodexSubscription).mockReset().mockResolvedValue(reading("expires"));
  const client = new QueryClient({defaultOptions:{queries:{retry:false}}});
  vi.useFakeTimers();
  const view = render(<QueryClientProvider client={client}><CodexSubscription accountId="idle-account" current /></QueryClientProvider>);
  try {
    await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60_000); });
    expect(api.readCodexSubscription).toHaveBeenCalledTimes(1);
    await act(async () => {
      await client.invalidateQueries({queryKey:["subscription", "codex"]});
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(api.readCodexSubscription).toHaveBeenCalledTimes(2);
  } finally {
    view.unmount();
    client.clear();
    vi.useRealTimers();
  }
});

it("shares saved metadata across remounts and checks only once a day", async () => {
  vi.mocked(api.readCodexSubscription).mockReset().mockResolvedValue(reading("expires"));
  const client = new QueryClient({defaultOptions:{queries:{retry:false}}});
  vi.useFakeTimers();
  const card = <QueryClientProvider client={client}><CodexSubscription accountId="daily-account" current /></QueryClientProvider>;
  let view = render(card);
  try {
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    view.unmount();
    view = render(card);
    await act(async () => { await vi.advanceTimersByTimeAsync(23 * 60 * 60_000); });
    expect(api.readCodexSubscription).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(60 * 60_000); });
    expect(api.readCodexSubscription).toHaveBeenCalledTimes(2);
  } finally {
    view.unmount(); client.clear(); vi.useRealTimers();
  }
});

import { SubscriptionStatus } from "./SubscriptionDate";
it("shows renewal, cancellation and expiry without inventing a completed renewal", () => {
  const now = Date.parse("2026-09-10T12:00:00Z");
  const {rerender} = render(<SubscriptionStatus reading={reading("renews")} now={now} />);
  expect(screen.getByText("Auto-renews")).toBeInTheDocument();
  rerender(<SubscriptionStatus reading={reading("expires")} now={now} />);
  expect(screen.getByText("No renewal")).toBeInTheDocument();
  const after = Date.parse("2026-10-10T12:00:00Z");
  rerender(<SubscriptionStatus reading={reading("expires")} now={after} />);
  expect(screen.getByText("Expired")).toBeInTheDocument();
  rerender(<SubscriptionStatus reading={reading("renews")} now={after} />);
  expect(screen.getByText("Unconfirmed")).toBeInTheDocument();
  expect(screen.queryByText("Expired")).not.toBeInTheDocument();
});
it("qualifies saved status and does not infer cancellation from a cached token", () => {
  const now = Date.parse("2026-09-10T12:00:00Z");
  const {rerender} = render(<SubscriptionStatus reading={reading("expires", true)} now={now} />);
  expect(screen.getByText("No renewal · cached")).toBeInTheDocument();
  rerender(<SubscriptionStatus reading={reading("paidThrough")} now={now} />);
  expect(screen.getByText("Active · cached")).toBeInTheDocument();
  rerender(<SubscriptionStatus reading={{metadata:null,connected:false,unavailable:false}} now={now} />);
  expect(screen.queryByLabelText(/Subscription status/)).not.toBeInTheDocument();
});
