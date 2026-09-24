import { TooltipButton } from "$lib/TooltipButton";
import { useCodexSubscription } from "$lib/useCodexSubscription";
import type { SubscriptionReading } from "$lib/subscriptionTypes";
import { claudeSubscriptionStatus } from "./claudeSubscriptionStatus";
import "./SubscriptionBadge.css";

const DAY = 86_400_000;
function exactDate(value: string) {
  return new Date(value).toLocaleString(undefined, {
    year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

export function SubscriptionBadge({ reading, now }: {reading: SubscriptionReading; now: number}) {
  const metadata = reading.metadata;
  const until = metadata?.date ? Date.parse(metadata.date) : NaN;
  if (!metadata || !Number.isFinite(until)) return null;
  const remaining = until - now;
  const billing = metadata.source === "billing";
  const expires = billing && metadata.kind === "expires";
  const renews = billing && metadata.kind === "renews";
  const elapsed = remaining <= 0;
  const progress = expires ? Math.max(0, Math.min(1, 1 - remaining / (7 * DAY))) : 0;
  const tone = expires && elapsed ? "expired" : expires && remaining <= 7 * DAY ? "warning" : "neutral";
  const label = expires ? "No renewal" : renews && !elapsed ? "Auto-renew" : "Unconfirmed";
  const verb = expires ? elapsed ? "Recorded expiry:" : "Expires" : renews ? elapsed ? "Renewal was due" : "Renews" : "Paid through";
  const days = Math.ceil(remaining / DAY);
  const countdown = remaining < DAY ? "Less than 1 day remaining" : `${days} day${days === 1 ? "" : "s"} remaining`;
  const tooltip = <>
    <div>{verb} {exactDate(metadata.date!)}</div>
    {expires && !elapsed && <div>{countdown}</div>}
    {!billing && <div>Renewal status unknown.</div>}
    {elapsed && <div>Current subscription status needs confirmation.</div>}
    {(metadata.stale || reading.unavailable) && <div>Last known billing information.</div>}
    {metadata.checkedAt && Number.isFinite(Date.parse(metadata.checkedAt)) && <div className="mt-1 text-[11px] text-[var(--mute)]">Last checked {exactDate(metadata.checkedAt)}</div>}
  </>;
  return <TooltipButton label={`Subscription status: ${label}`} tooltip={tooltip} className={`type-badge subscription-badge subscription-badge--${tone}`}>
    {tone === "warning" && <span aria-hidden="true" className="subscription-badge-fill" style={{width: `${progress * 100}%`, backgroundColor: `color-mix(in srgb, var(--expiry-fill-start), var(--expiry-fill-end) ${progress * 100}%)`}} />}
    <span className="relative">{label}</span>
  </TooltipButton>;
}

export function CodexSubscriptionBadge({accountId, current, now}: {accountId: string; current: boolean; now: number}) {
  const query = useCodexSubscription(accountId, current);
  return <SubscriptionBadge reading={query.data ?? {metadata: null, connected: false, unavailable: false}} now={now} />;
}

/**
 * Claude's own subscription status beside the plan, when it is anything but active. It carries no
 * dates: the OAuth token on-n-off holds is not given a renewal or expiry date (see PROVIDERS.md).
 */
export function ClaudeSubscriptionStatusBadge({ status, lastKnown }: { status: string | null | undefined; lastKnown: boolean }) {
  const badge = claudeSubscriptionStatus(status);
  if (!badge) return null;
  const tooltip = <>
    <div>Claude reports this subscription as {status?.trim()}</div>
    {lastKnown && <div>Last known subscription status.</div>}
  </>;
  return <TooltipButton label={`Subscription status: ${badge.label}`} tooltip={tooltip}
    className={`type-badge subscription-badge subscription-badge--${badge.tone === "alert" ? "expired" : "neutral"}`}>
    <span className="relative">{badge.label}</span>
  </TooltipButton>;
}
