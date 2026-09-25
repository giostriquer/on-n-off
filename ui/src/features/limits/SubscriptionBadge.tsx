import { TooltipButton } from "$lib/TooltipButton";
import { useCodexSubscription } from "$lib/useCodexSubscription";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { LimitsSubscription, ProviderLimits } from "$lib/limitsTypes";
import { claudeSubscriptionStatus } from "./claudeSubscriptionStatus";
import { codexSubscriptionTerm } from "./codexSubscriptionTerm";
import type { LimitAccountPresentation } from "./limitPresentation";
import "./SubscriptionBadge.css";

/** The Codex badge: `codexSubscriptionTerm` decides what it says; this only draws it. */
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
  return <TooltipButton label={shown.name} tooltip={tooltip} className={`type-badge subscription-badge subscription-badge--${shown.tone}`}>
    {shown.tone === "warning" && <span aria-hidden="true" className="subscription-badge-fill" style={{width: `${shown.progress * 100}%`, backgroundColor: `color-mix(in srgb, var(--expiry-fill-start), var(--expiry-fill-end) ${shown.progress * 100}%)`}} />}
    <span className="relative">{shown.label}</span>
  </TooltipButton>;
}

export function CodexSubscriptionBadge({accountId, current, term, now}: {accountId: string; current: boolean; term?: LimitsSubscription | null; now: number}) {
  const query = useCodexSubscription(accountId, current);
  return <SubscriptionBadge term={term} paidThrough={query.data ?? null} now={now} />;
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
    <span className="relative">{badge.label}</span>
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
      ? <CodexSubscriptionBadge accountId={entry.account.id} current={entry.currentAccount} term={entry.subscription} now={now} />
      : null;
  }
  if (entry.provider === "claude") {
    return <ClaudeSubscriptionStatusBadge status={entry.subscriptionStatus} lastKnown={freshness.lastKnown} checkedAt={freshness.updatedAt} />;
  }
  return null;
}
