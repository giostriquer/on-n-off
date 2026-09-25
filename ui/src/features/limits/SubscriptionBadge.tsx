import { TooltipButton } from "$lib/TooltipButton";
import { formatShortDate } from "$lib/limitsFormat";
import { useCodexSubscription } from "$lib/useCodexSubscription";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import { claudeSubscriptionStatus } from "./claudeSubscriptionStatus";
import type { LimitAccountPresentation } from "./limitPresentation";
import "./SubscriptionBadge.css";

function exactDate(value: string) {
  return new Date(value).toLocaleString(undefined, {
    year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

/**
 * How long a Codex plan is paid for, as its login's ID token says. The token carries no renewal or
 * cancellation status (see PROVIDERS.md), so the badge names the date and the tooltip says what it
 * does not know. A date that has passed shows nothing: a lapsed period says nothing about the
 * current one.
 */
export function SubscriptionBadge({ subscription, now }: { subscription: SubscriptionDate | null; now: number }) {
  const until = subscription ? Date.parse(subscription.date) : NaN;
  if (!subscription || !Number.isFinite(until) || until <= now) return null;
  const short = formatShortDate(subscription.date, { yearUnlessSameAs: now });
  const checked = subscription.checkedAt && Number.isFinite(Date.parse(subscription.checkedAt)) ? exactDate(subscription.checkedAt) : null;
  const tooltip = <>
    <div>Paid through {exactDate(subscription.date)}</div>
    <div>Renewal status unknown. The login says how long the plan is paid for, not whether it renews.</div>
    {checked && <div className="mt-1 text-[11px] text-[var(--mute)]">Confirmed by OpenAI {checked}</div>}
  </>;
  return <TooltipButton label={`Subscription paid through ${short}`} tooltip={tooltip} className="type-badge subscription-badge subscription-badge--neutral">
    Until {short}
  </TooltipButton>;
}

export function CodexSubscriptionBadge({accountId, current, now}: {accountId: string; current: boolean; now: number}) {
  const query = useCodexSubscription(accountId, current);
  return <SubscriptionBadge subscription={query.data ?? null} now={now} />;
}

/**
 * Claude's own subscription status beside the plan, when it is anything but active. It carries no
 * dates: the OAuth token on-n-off holds is not given a renewal or expiry date (see PROVIDERS.md).
 */
export function ClaudeSubscriptionStatusBadge({ status, lastKnown, checkedAt }: {
  status: string | null | undefined; lastKnown: boolean; checkedAt: string | null;
}) {
  const badge = claudeSubscriptionStatus(status);
  if (!badge) return null;
  const tooltip = <>
    <div>Claude reports this subscription as {status?.trim()}</div>
    {lastKnown && <div>Last known subscription status.</div>}
    {checkedAt && <div className="mt-1 text-[11px] text-[var(--mute)]">Checked {checkedAt}</div>}
  </>;
  return <TooltipButton label={`Subscription status: ${badge.label}`} tooltip={tooltip}
    className={`type-badge subscription-badge subscription-badge--${badge.tone}`}>
    {badge.label}
  </TooltipButton>;
}

/**
 * The subscription badge a card's header shows, whichever provider it is: Codex's paid-through
 * date, or Claude's status. `freshness` is the card's own presentation, so the badge says what the
 * card says about how current it is.
 */
export function AccountSubscriptionBadge({ entry, now, freshness }: {
  entry: ProviderLimits; now: number; freshness: Pick<LimitAccountPresentation, "lastKnown" | "updatedAt">;
}) {
  if (entry.provider === "codex") {
    return entry.account
      ? <CodexSubscriptionBadge accountId={entry.account.id} current={entry.currentAccount} now={now} />
      : null;
  }
  if (entry.provider === "claude") {
    return <ClaudeSubscriptionStatusBadge status={entry.subscriptionStatus} lastKnown={freshness.lastKnown} checkedAt={freshness.updatedAt} />;
  }
  return null;
}
