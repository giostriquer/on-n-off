import { useCodexSubscription } from "$lib/useCodexSubscription";
import type { SubscriptionReading } from "$lib/subscriptionTypes";

function dateLabel(value: string | null, now?: number) {
  if (!value) return null;
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toLocaleDateString(undefined, {
    year: now === undefined || date.getFullYear() !== new Date(now).getFullYear() ? "numeric" : undefined,
    month: "short", day: "numeric",
  }) : null;
}

export function SubscriptionDate({ reading, now = Date.now(), className = "" }: { reading: SubscriptionReading; now?: number; className?: string }) {
  const metadata = reading.metadata;
  const date = dateLabel(metadata?.date ?? null, now);
  if (!date || !metadata) return null;
  const checked = dateLabel(metadata.checkedAt);
  const remaining = Date.parse(metadata.date!) - now;
  const elapsed = remaining <= 0;
  const days = Math.ceil(remaining / 86_400_000);
  const countdown = remaining < 86_400_000 ? "<1 day" : `${days} day${days === 1 ? "" : "s"}`;
  const text = metadata.kind === "renews" ? `${elapsed ? "Renewal was due" : "Renews"} ${date}`
    : metadata.kind === "expires" ? elapsed ? `Expired on ${date}` : `Expires in ${countdown} · ${date}`
    : `Paid through ${date}`;
  const urgent = metadata.kind === "expires" && remaining <= 7 * 86_400_000;
  const detail = [checked ? `Last checked ${checked}.` : "", reading.unavailable ? "Billing could not be checked." : "", metadata.source === "localToken" ? "Renewal status unknown." : ""].filter(Boolean).join(" ");
  return <span className={`mt-0.5 block text-[11px] leading-relaxed ${urgent ? "text-[var(--warn)]" : "text-[var(--mute)]"} ${className}`} title={detail || undefined}>
    {text}
  </span>;
}

export function CodexSubscription({ accountId, current, now = Date.now(), className }: { accountId: string; current: boolean; now?: number; className?: string }) {
  const query = useCodexSubscription(accountId, current);
  const reading = query.data ?? { metadata: null, connected: false, unavailable: false };
  return <SubscriptionDate reading={reading} now={now} className={className} />;
}
