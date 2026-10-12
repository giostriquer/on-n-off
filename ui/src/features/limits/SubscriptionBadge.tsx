import { useCodexSubscription } from "$lib/useCodexSubscription";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { LimitsSubscription } from "$lib/limitsTypes";
import { codexSubscriptionTerm } from "./codexSubscriptionTerm";
import type { CardSubscription } from "./limitCards";
import { TermBadge } from "./TermBadge";

export function SubscriptionBadge({ term, paidThrough, now }: { term?: LimitsSubscription | null; paidThrough: SubscriptionDate | null; now: number }) {
  const shown = codexSubscriptionTerm(term, paidThrough, now);
  if (!shown) return null;
  const tooltip = <>
    <div>{shown.headline}</div>
    {shown.countdown && <div>{shown.countdown}</div>}
    {shown.note && <div>{shown.note}</div>}
    {shown.caveat && <div>{shown.caveat}</div>}
    {shown.checked && <div className="mt-1 text-[11px] text-[var(--mute)]">{shown.checked}</div>}
  </>;
  return <TermBadge badge={shown} tooltip={tooltip} />;
}

export function CodexSubscriptionBadge({accountId, current, term, now}: {accountId: string; current: boolean; term?: LimitsSubscription | null; now: number}) {
  const query = useCodexSubscription(accountId, current);
  return <SubscriptionBadge term={term} paidThrough={query.data ?? null} now={now} />;
}

export function AccountSubscriptionBadge({ subscription, now }: { subscription: CardSubscription | null; now: number }) {
  if (!subscription) return null;
  return <CodexSubscriptionBadge accountId={subscription.accountId} current={subscription.current} term={subscription.term} now={now} />;
}
