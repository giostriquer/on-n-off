import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SubscriptionReading } from "$lib/subscriptionTypes";
import { ClaudeSubscriptionStatusBadge, SubscriptionBadge } from "./SubscriptionBadge";
vi.mock("$lib/api", () => ({}));
afterEach(cleanup);
const DAY = 86_400_000;
const NOW = Date.parse("2026-09-14T12:00:00Z");
function reading(days: number, kind: "renews" | "expires" | "paidThrough" = "expires"): SubscriptionReading {
  return {connected: false, unavailable: false, metadata: {
    date: new Date(NOW + days * DAY).toISOString(), kind, source: kind === "paidThrough" ? "localToken" : "billing",
    checkedAt: new Date(NOW).toISOString(), stale: false,
  }};
}
it("fills the seven-day warning window from elapsed time, without treating renewal as a warning", () => {
  const value = reading(7);
  const {container, rerender} = render(<SubscriptionBadge reading={value} now={NOW} />);
  expect(container.querySelector<HTMLElement>(".subscription-badge-fill")!.style.width).toBe("0%");
  rerender(<SubscriptionBadge reading={value} now={NOW + 3.5 * DAY} />);
  expect(container.querySelector<HTMLElement>(".subscription-badge-fill")!.style.width).toBe("50%");
  rerender(<SubscriptionBadge reading={value} now={NOW + 6.3 * DAY} />);
  expect(parseFloat(container.querySelector<HTMLElement>(".subscription-badge-fill")!.style.width)).toBeCloseTo(90);
  rerender(<SubscriptionBadge reading={reading(1, "renews")} now={NOW} />);
  expect(screen.getByRole("button", {name: /Auto-renew/})).toHaveClass("subscription-badge--neutral");
  expect(container.querySelector(".subscription-badge-fill")).toBeNull();
});
it("keeps distant expiry quiet and marks the passed date without claiming verified access loss", () => {
  const {rerender} = render(<SubscriptionBadge reading={reading(30)} now={NOW} />);
  expect(screen.getByRole("button")).toHaveClass("subscription-badge--neutral");
  rerender(<SubscriptionBadge reading={reading(0)} now={NOW} />);
  const badge = screen.getByRole("button");
  expect(badge).toHaveClass("subscription-badge--expired");
  fireEvent.focus(badge);
  expect(screen.getByRole("tooltip")).toHaveTextContent("Recorded expiry:");
  expect(screen.getByRole("tooltip")).toHaveTextContent("Current subscription status needs confirmation.");
});
it("reveals the device-local clock time without a timezone suffix, remaining time, and retained-data caveat", () => {
  const value = reading(0.5); value.unavailable = true;
  render(<SubscriptionBadge reading={value} now={NOW} />);
  fireEvent.focus(screen.getByRole("button"));
  const tip = screen.getByRole("tooltip");
  expect(tip).toHaveTextContent(/2026/);
  expect(tip).toHaveTextContent(/\d{1,2}:\d{2}/);
  expect(tip).not.toHaveTextContent(/GMT|UTC/);
  expect(tip).toHaveTextContent("Less than 1 day remaining");
  expect(tip).toHaveTextContent("Last known billing information.");
});
it.each([reading(-1, "renews"), reading(10, "paidThrough")])("never infers renewal from an overdue date or a token fallback", value => {
  render(<SubscriptionBadge reading={value} now={NOW} />);
  expect(screen.getByRole("button", {name: /Unconfirmed/})).toHaveClass("subscription-badge--neutral");
});
it.each([null, "invalid"])("omits a badge when its date is %s", date => {
  const value = reading(4); value.metadata!.date = date;
  const {container} = render(<SubscriptionBadge reading={value} now={NOW} />);
  expect(container).toBeEmptyDOMElement();
});
describe("ClaudeSubscriptionStatusBadge", () => {
  it("names what Claude reports, in the red tone when the subscription is in trouble", () => {
    render(<ClaudeSubscriptionStatusBadge status="past_due" lastKnown={false} />);
    const badge = screen.getByRole("button", { name: "Subscription status: Payment due" });
    expect(badge).toHaveClass("subscription-badge--expired");
    expect(badge).toHaveTextContent("Payment due");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Claude reports this subscription as past_due");
    expect(screen.getByRole("tooltip")).not.toHaveTextContent("Last known");
  });
  it("keeps a trial neutral and says when the status is only the last one known", () => {
    render(<ClaudeSubscriptionStatusBadge status="trialing" lastKnown />);
    const badge = screen.getByRole("button", { name: "Subscription status: Trial" });
    expect(badge).toHaveClass("subscription-badge--neutral");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Last known subscription status.");
  });
  it("shows nothing for an active or unknown subscription", () => {
    const { container, rerender } = render(<ClaudeSubscriptionStatusBadge status="active" lastKnown={false} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<ClaudeSubscriptionStatusBadge status={null} lastKnown={false} />);
    expect(container).toBeEmptyDOMElement();
  });
});
