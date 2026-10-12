import { formatExactDate, formatShortDate } from "$lib/limitsFormat";
import type { LimitsSubscription } from "$lib/limitsTypes";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { TermTone } from "./TermBadge";
import { TERM_WARNING_WINDOW_MS, termCountdown } from "./termCountdown";

export type TermState = "renews" | "ends" | "ended" | "renewalOverdue" | "tokenOnly";

export type TermPresentation = {
  state: TermState;
  name: string;
  label: string;
  tone: TermTone;
  progress: number;
  headline: string;
  countdown: string | null;
  note: string | null;
  caveat: string | null;
  checked: string | null;
};

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
  const ending = state === "ends" ? termCountdown(remaining) : null;
  const label = state === "ends" || state === "ended" ? "No renewal" : state === "renews" ? "Auto-renew" : state === "renewalOverdue" ? "Unconfirmed" : `Until ${short}`;
  const checkedAt = term ? term.checkedAt : paidThrough?.checkedAt;
  const checkedLine = formatExactDate(checkedAt) || null;
  return {
    state,
    name: term ? `Subscription status: ${label}` : `Subscription paid through ${short}`,
    label,
    tone: state === "ended" ? "expired" : ending && remaining <= TERM_WARNING_WINDOW_MS ? "warning" : "neutral",
    progress: ending?.progress ?? 0,
    headline: `${VERBS[state]} ${formatExactDate(date)}`,
    countdown: ending?.countdown ?? null,
    note: term?.note ? NOTES[term.note] : null,
    caveat: state === "tokenOnly" ? "Renewal status unknown. The login says how long the plan is paid for, not whether it renews."
      : state === "ended" || state === "renewalOverdue" ? "Current subscription status needs confirmation." : null,
    checked: checkedLine ? (term ? `Checked ${checkedLine}` : `Confirmed by OpenAI ${checkedLine}`) : null,
  };
}

export function codexTermEnded(term: LimitsSubscription | null | undefined, now: number): boolean {
  return !!term && codexSubscriptionTerm(term, null, now)?.state === "ended";
}
