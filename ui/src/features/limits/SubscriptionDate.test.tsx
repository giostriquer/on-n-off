import { render, screen, cleanup, act } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { SubscriptionDate } from "./SubscriptionDate";
import type { SubscriptionReading } from "$lib/subscriptionTypes";
afterEach(cleanup);
const NOW = Date.parse("2026-09-13T12:00:00Z");
function reading(kind: "renews" | "expires" | "paidThrough", stale = false): SubscriptionReading {
  return { connected: false, unavailable: false, metadata: { date: "2026-10-10T12:00:00Z", kind, source: kind === "paidThrough" ? "localToken" : "billing", checkedAt: "2026-09-10T12:00:00Z", stale } };
}
it.each([
  ["2026-10-06T12:00:00Z", "Expires in 4 days"],
  ["2026-10-09T12:00:00Z", "Expires in 1 day"],
  ["2026-10-10T11:00:00Z", "Expires in <1 day"],
])("shows remaining subscription time at %s", (now, countdown) => {
  render(<SubscriptionDate reading={reading("expires")} now={Date.parse(now)} />);
  expect(screen.getByText(/Expires in/)).toHaveTextContent(countdown);
});
it("updates the expiry countdown and stops it when the subscription expires", () => {
  const {rerender}=render(<SubscriptionDate reading={reading("expires")} now={Date.parse("2026-10-08T12:00:00Z")} />);
  expect(screen.getByText(/Expires in/)).toHaveTextContent("Expires in 2 days");
  rerender(<SubscriptionDate reading={reading("expires")} now={Date.parse("2026-10-09T12:00:00Z")} />);
  expect(screen.getByText(/Expires in/)).toHaveTextContent("Expires in 1 day");
  rerender(<SubscriptionDate reading={reading("expires")} now={Date.parse("2026-10-10T12:00:00Z")} />);
  expect(screen.getByText(/Expired on/)).not.toHaveTextContent(/Expires in/);
});
it.each(["renews", "paidThrough"] as const)("does not label %s as time left before expiry", kind => {
  render(<SubscriptionDate reading={reading(kind)} now={Date.parse("2026-10-06T12:00:00Z")} />);
  expect(screen.queryByText(/Expires in/)).toBeNull();
});
it("distinguishes cancellation from renewal", () => {
  const { rerender } = render(<SubscriptionDate reading={reading("expires")} now={NOW} />);
  expect(screen.getByText(/Expires in/)).toHaveTextContent(/Oct/);
  expect(screen.queryByText(/Renews/)).not.toBeInTheDocument();
  rerender(<SubscriptionDate reading={reading("renews")} now={NOW} />);
  expect(screen.getByText(/Renews/)).toBeInTheDocument();
});
it("shows paid-through dates without cache jargon or implied cancellation", () => {
  render(<SubscriptionDate reading={reading("paidThrough", true)} now={NOW} />);
  expect(screen.getByText(/Paid through/)).not.toHaveTextContent(/cached|stale/i);
  expect(screen.getByText(/Paid through/)).toHaveAttribute("title", expect.stringContaining("Last checked"));
  expect(screen.queryByText(/Expires|Expired|Renews/)).not.toBeInTheDocument();
});
it("keeps retained billing dates concise after failure", () => {
  render(<SubscriptionDate reading={{ ...reading("expires", true), unavailable: true }} now={NOW} />);
  expect(screen.getByText(/Expires in/)).not.toHaveTextContent(/stale|cached/i);
  expect(screen.getByText(/Expires in/)).toHaveAttribute("title", expect.stringContaining("Billing could not be checked"));
});
it("does not invent a date for an empty billing observation", () => {
  const value = reading("renews"); value.metadata!.date = null; value.metadata!.kind = null;
  render(<SubscriptionDate reading={value} />);
  expect(screen.queryByText(/Subscription date unavailable/)).not.toBeInTheDocument();
});

// IPC is the boundary: a normal render must not access browser cookies.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { vi } from "vitest";
import * as api from "$lib/api";
import { CodexSubscription } from "./SubscriptionDate";
vi.mock("$lib/api", () => ({readCodexSubscription:vi.fn(),connectCodexBilling:vi.fn()}));
vi.mock("$lib/useSharedRead", () => ({useSharedRead:vi.fn()}));
it("renders a passive date without an interactive browser import on the Limits card", async () => {
  vi.mocked(api.readCodexSubscription).mockResolvedValue({...reading("paidThrough"),browserSupported:true});
  vi.mocked(api.connectCodexBilling).mockClear().mockResolvedValue();
  const client = new QueryClient({defaultOptions:{queries:{retry:false}}});
  render(<QueryClientProvider client={client}><CodexSubscription accountId="account-a" current /></QueryClientProvider>);
  await screen.findByText(/Paid through/);
  expect(api.connectCodexBilling).not.toHaveBeenCalled();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
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

it("rechecks eligibility after returning from a route where account discovery ran", async () => {
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
    expect(api.readCodexSubscription).toHaveBeenCalledTimes(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(60 * 60_000); });
    expect(api.readCodexSubscription).toHaveBeenCalledTimes(3);
  } finally {
    view.unmount(); client.clear(); vi.useRealTimers();
  }
});

it("marks only near expiry dates as urgent and preserves overdue renewal uncertainty", () => {
  const {rerender} = render(<SubscriptionDate reading={reading("expires")} now={Date.parse("2026-10-06T12:00:00Z")} />);
  expect(screen.getByText(/Expires in/)).toHaveClass("text-[var(--warn)]");
  rerender(<SubscriptionDate reading={reading("expires")} now={Date.parse("2026-09-10T12:00:00Z")} />);
  expect(screen.getByText(/Expires in/)).toHaveClass("text-[var(--mute)]");
  rerender(<SubscriptionDate reading={reading("renews")} now={Date.parse("2026-10-11T12:00:00Z")} />);
  expect(screen.getByText(/Renewal was due/)).toBeVisible();
  expect(screen.queryByText(/Expires|Expired/)).toBeNull();
});
it("omits invalid dates and retains the year for dates outside the current year", () => {
  const value = reading("expires"); value.metadata!.date = "invalid";
  const {container, rerender} = render(<SubscriptionDate reading={value} />);
  expect(container).toBeEmptyDOMElement();
  rerender(<SubscriptionDate reading={reading("renews")} now={Date.parse("2025-12-01T12:00:00Z")} />);
  expect(screen.getByText(/Renews/)).toHaveTextContent("2026");
});

it("does not diagnose a wrong login from unavailable billing", () => {
  render(<SubscriptionDate reading={{ ...reading("paidThrough", true), unavailable: true }} now={NOW} />);
  expect(screen.queryByText(/Sign in to the matching ChatGPT account/)).not.toBeInTheDocument();
  expect(screen.getByText(/Paid through/)).toHaveAttribute("title", expect.stringContaining("Billing could not be checked"));
});

it("shows the date directly without a separate Billing menu",()=>{
 const {container}=render(<SubscriptionDate reading={reading("expires")} now={NOW} />);
 expect(screen.getByText(/Expires in/)).toBeVisible();
 expect(container.querySelector("details")).toBeNull();
 expect(screen.queryByText("Billing")).not.toBeInTheDocument();
});
