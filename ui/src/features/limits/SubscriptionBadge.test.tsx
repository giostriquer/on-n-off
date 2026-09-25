import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { LimitsSubscription } from "$lib/limitsTypes";
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
    const { rerender } = render(<SubscriptionBadge paidThrough={subscription(30)} now={NOW} />);
    const badge = screen.getByRole("button", { name: "Subscription paid through Oct 14" });
    expect(badge).toHaveTextContent("Until Oct 14");
    expect(badge).toHaveClass("subscription-badge--neutral");
    rerender(<SubscriptionBadge paidThrough={subscription(1)} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription paid through Sep 15" })).toHaveClass("subscription-badge--neutral");
    rerender(<SubscriptionBadge paidThrough={subscription(200)} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription paid through Apr 2, 2027" })).toHaveTextContent("Until Apr 2, 2027");
  });
  it("tells the exact device-local time, that renewal is unknown, and when OpenAI last confirmed the date", () => {
    render(<SubscriptionBadge paidThrough={subscription(10)} now={NOW} />);
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
    render(<SubscriptionBadge paidThrough={subscription(10, null)} now={NOW} />);
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
    const { container } = render(<SubscriptionBadge paidThrough={value} now={NOW} />);
    expect(container).toBeEmptyDOMElement();
  });
});
function term(days: number, willRenew: boolean, note: LimitsSubscription["note"] = null): LimitsSubscription {
  return { activeUntil: new Date(NOW + days * DAY).toISOString(), willRenew, note, checkedAt: new Date(NOW - DAY).toISOString() };
}
describe("SubscriptionBadge with the billing term", () => {
  it("fills the seven-day warning window from elapsed time, without treating renewal as a warning", () => {
    const ending = term(7, false);
    const { container, rerender } = render(<SubscriptionBadge term={ending} paidThrough={null} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription status: No renewal" })).toHaveClass("subscription-badge--warning");
    expect(container.querySelector<HTMLElement>(".subscription-badge-fill")!.style.width).toBe("0%");
    rerender(<SubscriptionBadge term={ending} paidThrough={null} now={NOW + 3.5 * DAY} />);
    expect(container.querySelector<HTMLElement>(".subscription-badge-fill")!.style.width).toBe("50%");
    rerender(<SubscriptionBadge term={term(1, true)} paidThrough={null} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription status: Auto-renew" })).toHaveClass("subscription-badge--neutral");
    expect(container.querySelector(".subscription-badge-fill")).toBeNull();
  });
  it("keeps a distant end quiet and marks a passed one without claiming verified access loss", () => {
    const { rerender } = render(<SubscriptionBadge term={term(30, false)} paidThrough={null} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription status: No renewal" })).toHaveClass("subscription-badge--neutral");
    rerender(<SubscriptionBadge term={term(0, false, "cancelled")} paidThrough={null} now={NOW} />);
    const badge = screen.getByRole("button", { name: "Subscription status: No renewal" });
    expect(badge).toHaveClass("subscription-badge--expired");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Recorded expiry:");
    expect(screen.getByRole("tooltip")).toHaveTextContent("Cancelled: the plan ends with this period.");
    expect(screen.getByRole("tooltip")).toHaveTextContent("Current subscription status needs confirmation.");
  });
  it("tells the exact time, the days left, the note and when it was checked", () => {
    render(<SubscriptionBadge term={term(0.5, false, "planChange")} paidThrough={null} now={NOW} />);
    fireEvent.focus(screen.getByRole("button"));
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent(/Expires .*2026/);
    expect(tip).toHaveTextContent(/\d{1,2}:\d{2}/);
    expect(tip).toHaveTextContent("Less than 1 day remaining");
    expect(tip).toHaveTextContent("Another plan takes over at the end of this period.");
    expect(tip).toHaveTextContent(/Checked .*2026/);
    expect(tip).not.toHaveTextContent(/Renewal status unknown|Confirmed by OpenAI/);
  });
  it("never infers renewal from an overdue date, and says why a renewing plan is in trouble", () => {
    render(<SubscriptionBadge term={term(-1, true, "pastDue")} paidThrough={null} now={NOW} />);
    const badge = screen.getByRole("button", { name: "Subscription status: Unconfirmed" });
    expect(badge).toHaveClass("subscription-badge--neutral");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Renewal was due");
    expect(screen.getByRole("tooltip")).toHaveTextContent("A payment is overdue.");
  });
  it("prefers the billing term to the token date, and falls back to the token when there is none", () => {
    const { rerender } = render(<SubscriptionBadge term={term(3, true)} paidThrough={subscription(40)} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription status: Auto-renew" })).toBeTruthy();
    rerender(<SubscriptionBadge term={null} paidThrough={subscription(40)} now={NOW} />);
    expect(screen.getByRole("button", { name: "Subscription paid through Oct 24" })).toBeTruthy();
    rerender(<SubscriptionBadge term={{ ...term(3, true), activeUntil: "invalid" }} paidThrough={null} now={NOW} />);
    expect(screen.queryByRole("button")).toBeNull();
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
