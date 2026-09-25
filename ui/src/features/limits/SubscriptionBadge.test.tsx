import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import { ClaudeSubscriptionStatusBadge, SubscriptionBadge } from "./SubscriptionBadge";
vi.mock("$lib/api", () => ({}));
afterEach(cleanup);
const DAY = 86_400_000;
const NOW = Date.parse("2026-09-14T12:00:00Z");
function subscription(days: number, checkedAt: string | null = new Date(NOW - DAY).toISOString()): SubscriptionDate {
  return { date: new Date(NOW + days * DAY).toISOString(), checkedAt };
}
describe("SubscriptionBadge", () => {
  it("names the paid-through date in the neutral tone, near or far, with the year only when it differs", () => {
    const { rerender } = render(<SubscriptionBadge subscription={subscription(30)} now={NOW} />);
    const badge = screen.getByRole("button", { name: "Subscription paid through Oct 14" });
    expect(badge).toHaveTextContent("Until Oct 14");
    expect(badge).toHaveClass("subscription-badge--neutral");
    rerender(<SubscriptionBadge subscription={subscription(1)} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription paid through Sep 15" })).toHaveClass("subscription-badge--neutral");
    rerender(<SubscriptionBadge subscription={subscription(200)} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription paid through Apr 2, 2027" })).toHaveTextContent("Until Apr 2, 2027");
  });
  it("tells the exact device-local time, that renewal is unknown, and when OpenAI last confirmed the date", () => {
    render(<SubscriptionBadge subscription={subscription(10)} now={NOW} />);
    fireEvent.focus(screen.getByRole("button"));
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent(/Paid through .*2026/);
    expect(tip).toHaveTextContent(/\d{1,2}:\d{2}/);
    expect(tip).not.toHaveTextContent(/GMT|UTC/);
    expect(tip).toHaveTextContent("Renewal status unknown.");
    expect(tip).toHaveTextContent(/Confirmed by OpenAI .*2026/);
    expect(tip).not.toHaveTextContent(/Auto-renew|No renewal|Expires|cancel/i);
  });
  it("claims no confirmation when the token carries no check time", () => {
    render(<SubscriptionBadge subscription={subscription(10, null)} now={NOW} />);
    fireEvent.focus(screen.getByRole("button"));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Paid through");
    expect(screen.getByRole("tooltip")).not.toHaveTextContent("Confirmed");
  });
  it.each([
    ["missing", null],
    ["invalid", { date: "invalid", checkedAt: null }],
    ["passed", subscription(-1)],
    ["ending this instant", subscription(0)],
  ])("omits the badge when the date is %s", (_case, value) => {
    const { container } = render(<SubscriptionBadge subscription={value} now={NOW} />);
    expect(container).toBeEmptyDOMElement();
  });
});
describe("ClaudeSubscriptionStatusBadge", () => {
  it("names what Claude reports, in the alert tone when the subscription is in trouble, and when it was checked", () => {
    render(<ClaudeSubscriptionStatusBadge status="past_due" lastKnown={false} checkedAt="Aug 17, 2026, 20:00" />);
    const badge = screen.getByRole("button", { name: "Subscription status: Payment due" });
    expect(badge).toHaveClass("subscription-badge--alert");
    expect(badge).toHaveTextContent("Payment due");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Claude reports this subscription as past_due");
    expect(screen.getByRole("tooltip")).toHaveTextContent("Checked Aug 17, 2026, 20:00");
    expect(screen.getByRole("tooltip")).not.toHaveTextContent("Last known");
  });
  it("keeps a trial neutral and says when the status is only the last one known", () => {
    render(<ClaudeSubscriptionStatusBadge status="trialing" lastKnown checkedAt={null} />);
    const badge = screen.getByRole("button", { name: "Subscription status: Trial" });
    expect(badge).toHaveClass("subscription-badge--neutral");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Last known subscription status.");
    expect(screen.getByRole("tooltip")).not.toHaveTextContent("Checked");
  });
  it("shows nothing for an active or unknown subscription", () => {
    const { container, rerender } = render(<ClaudeSubscriptionStatusBadge status="active" lastKnown={false} checkedAt={null} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<ClaudeSubscriptionStatusBadge status={null} lastKnown={false} checkedAt={null} />);
    expect(container).toBeEmptyDOMElement();
  });
});
