import { formatShortDate } from "$lib/limitsFormat";
import type { LimitsSubscription } from "$lib/limitsTypes";
import type { SubscriptionDate } from "$lib/subscriptionTypes";

const DAY = 86_400_000;

export type TermState = "renews" | "ends" | "ended" | "renewalOverdue" | "tokenOnly";

/** Everything the Codex subscription badge renders, decided once from its two inputs. */
export type TermPresentation = {
  state: TermState;
  /** The accessible name of the badge. */
  name: string;
  label: string;
  tone: "neutral" | "warning" | "expired";
  /** How far through its last seven days a plan that ends is, 0 to 1; 0 otherwise. */
  progress: number;
  /** The tooltip's first line: the verb and the exact device-local date. */
  headline: string;
  countdown: string | null;
  note: string | null;
  caveat: string | null;
  checked: string | null;
};

function exactDate(value: string) {
  return new Date(value).toLocaleString(undefined, {
    year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

const NOTES: Record<NonNullable<LimitsSubscription["note"]>, string> = {
  cancelled: "Cancelled: the plan ends with this period.",
  planChange: "Another plan takes over at the end of this period.",
  pastDue: "A payment is overdue.",
};

const VERBS: Record<TermState, string> = {
  renews: "Renews",
  ends: "Expires",
  ended: "Recorded expiry:",
  renewalOverdue: "Renewal was due",
  tokenOnly: "Paid through",
};

/**
 * The badge's state. The billing endpoint's answer (`term`) names the end of the paid period and
 * whether it renews then: a plan that ends is the one to spend down first, so it warns through its
 * last seven days and turns red once the date has passed. Without a term, the login token's date
 * (`paidThrough`) says how long the plan is paid for and nothing about renewal, and drops out once
 * it has passed: a lapsed period says nothing about the current one.
 */
export function codexSubscriptionTerm(
  term: LimitsSubscription | null | undefined,
  paidThrough: SubscriptionDate | null | undefined,
  now: number,
): TermPresentation | null {
  const date = term?.activeUntil ?? paidThrough?.date ?? null;
  const until = date ? Date.parse(date) : NaN;
  if (!date || !Number.isFinite(until)) return null;
  const remaining = until - now;
  const elapsed = remaining <= 0;
  const state: TermState = !term ? "tokenOnly" : !term.willRenew ? (elapsed ? "ended" : "ends") : elapsed ? "renewalOverdue" : "renews";
  if (state === "tokenOnly" && elapsed) return null;
  const short = formatShortDate(date, { yearUnlessSameAs: now });
  const days = Math.ceil(remaining / DAY);
  const label = state === "ends" || state === "ended" ? "No renewal" : state === "renews" ? "Auto-renew" : state === "renewalOverdue" ? "Unconfirmed" : `Until ${short}`;
  const checkedAt = term ? term.checkedAt : paidThrough?.checkedAt;
  const checkedLine = checkedAt && Number.isFinite(Date.parse(checkedAt)) ? exactDate(checkedAt) : null;
  return {
    state,
    name: term ? `Subscription status: ${label}` : `Subscription paid through ${short}`,
    label,
    tone: state === "ended" ? "expired" : state === "ends" && remaining <= 7 * DAY ? "warning" : "neutral",
    progress: state === "ends" ? Math.max(0, Math.min(1, 1 - remaining / (7 * DAY))) : 0,
    headline: `${VERBS[state]} ${exactDate(date)}`,
    countdown: state === "ends" ? (remaining < DAY ? "Less than 1 day remaining" : `${days} day${days === 1 ? "" : "s"} remaining`) : null,
    note: term?.note ? NOTES[term.note] : null,
    caveat: state === "tokenOnly" ? "Renewal status unknown. The login says how long the plan is paid for, not whether it renews."
      : state === "ended" || state === "renewalOverdue" ? "Current subscription status needs confirmation." : null,
    checked: checkedLine ? (term ? `Checked ${checkedLine}` : `Confirmed by OpenAI ${checkedLine}`) : null,
  };
}
