/**
 * How a Claude card names its subscription status, from the `organization.subscription_status`
 * the profile read reports. An active subscription needs no badge; the states that stop or will stop
 * the subscription are red; a trial is neutral. A state this list does not know is shown as written,
 * humanized, so a new one surfaces rather than disappearing. Nothing when the status is unknown.
 *
 * `ClaudeStatusLabel` is a status's wording and the `subscription-badge--<tone>` modifier it is
 * drawn with.
 */
export type ClaudeStatusLabel = { label: string; tone: "alert" | "neutral" };

const KNOWN: Record<string, ClaudeStatusLabel | null> = {
  active: null,
  past_due: { label: "Payment due", tone: "alert" },
  unpaid: { label: "Payment due", tone: "alert" },
  canceled: { label: "Canceled", tone: "alert" },
  cancelled: { label: "Canceled", tone: "alert" },
  expired: { label: "Expired", tone: "alert" },
  trialing: { label: "Trial", tone: "neutral" },
};

/**
 * Whether the status says the subscription has already ended: only `expired` does. A cancellation
 * may still run to the end of its period, which the status does not date, and a payment that is
 * due has not ended anything yet.
 */
export function claudeSubscriptionEnded(status: string | null | undefined): boolean {
  return statusKey(status) === "expired";
}

function statusKey(status: string | null | undefined): string {
  return status?.trim().toLowerCase() ?? "";
}

export function claudeSubscriptionStatus(status: string | null | undefined): ClaudeStatusLabel | null {
  const key = statusKey(status);
  if (!key) return null;
  if (Object.hasOwn(KNOWN, key)) return KNOWN[key];
  const words = key.replace(/_/g, " ");
  return { label: words.charAt(0).toUpperCase() + words.slice(1), tone: "neutral" };
}
