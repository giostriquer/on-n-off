import { describe, expect, it } from "vitest";
import { claudeSubscriptionStatus } from "./claudeSubscriptionStatus";

describe("claudeSubscriptionStatus", () => {
  it.each([
    ["past_due", "Payment due", "alert"],
    ["unpaid", "Payment due", "alert"],
    ["canceled", "Canceled", "alert"],
    ["cancelled", "Canceled", "alert"],
    ["expired", "Expired", "alert"],
    ["trialing", "Trial", "neutral"],
    ["PAST_DUE", "Payment due", "alert"],
    [" canceled ", "Canceled", "alert"],
  ] as const)("words %j as %s", (status, label, tone) => {
    expect(claudeSubscriptionStatus(status)).toEqual({ label, tone });
  });

  it("shows no badge for an active subscription", () => {
    expect(claudeSubscriptionStatus("active")).toBeNull();
    expect(claudeSubscriptionStatus(" Active ")).toBeNull();
  });

  it("shows no badge when the status is unknown", () => {
    expect(claudeSubscriptionStatus(undefined)).toBeNull();
    expect(claudeSubscriptionStatus(null)).toBeNull();
    expect(claudeSubscriptionStatus("  ")).toBeNull();
  });

  it("shows a state it does not know rather than hiding it", () => {
    expect(claudeSubscriptionStatus("incomplete_expired")).toEqual({ label: "Incomplete expired", tone: "neutral" });
  });
});
