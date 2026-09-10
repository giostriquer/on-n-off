import { useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { useSharedRead } from "$lib/useSharedRead";
import type { SubscriptionReading } from "$lib/subscriptionTypes";

function dateLabel(value: string | null) {
  if (!value) return null;
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toLocaleDateString(undefined, {
    year: "numeric", month: "short", day: "numeric",
  }) : null;
}

export function SubscriptionStatus({ reading, now }: { reading: SubscriptionReading; now: number }) {
  const metadata = reading.metadata;
  if (!metadata?.date) return null;
  const until = Date.parse(metadata.date);
  if (!Number.isFinite(until)) return null;
  const billing = metadata.source === "billing";
  const elapsed = until <= now;
  const label = billing && metadata.kind === "expires" ? (elapsed ? "Expired" : "No renewal")
    : billing && metadata.kind === "renews" && !elapsed ? "Auto-renews"
    : !billing && !elapsed ? "Active" : "Unconfirmed";
  const cached = metadata.stale || reading.unavailable || !billing;
  const detail = label === "No renewal" ? "Auto-renewal is off; access continues until the expiry date."
    : label === "Expired" ? "The recorded expiry date has passed and auto-renewal was off."
    : label === "Auto-renews" ? "Auto-renewal was enabled at the last billing check."
    : label === "Active" ? "Cached paid-through date; renewal status is unknown."
    : "The recorded date has passed. Refresh billing to confirm the current status.";
  return <span aria-label={`Subscription status: ${label}${cached ? " (cached)" : ""}`}
    title={`${cached ? "Last known status. " : ""}${detail}`}
    className={`shrink-0 whitespace-nowrap rounded-md border border-[var(--hair)] px-1.5 py-0.5 type-badge uppercase ${label === "No renewal" || label === "Expired" ? "text-[var(--warn)]" : "text-[var(--mute)]"}`}>
    {label}{cached ? " · cached" : ""}
  </span>;
}

export function SubscriptionDate({ reading, children, now = Date.now() }: { reading: SubscriptionReading; children?: ReactNode; now?: number }) {
  const metadata = reading.metadata;
  const date = dateLabel(metadata?.date ?? null);
  const checked = dateLabel(metadata?.checkedAt ?? null);
  const elapsed = metadata?.date ? Date.parse(metadata.date) <= now : false;
  const label = metadata?.kind === "renews" ? (elapsed ? "Renewal was due" : "Renews on") : metadata?.kind === "expires" ? (elapsed ? "Expired on" : "Expires on") : "Paid through";
  const qualifier = metadata?.source === "localToken" ? " · cached" : metadata?.stale ? " · stale" : "";
  return (
    <details className="group min-w-0 text-[11px] leading-relaxed text-[var(--mute)]">
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)] [&::-webkit-details-marker]:hidden">
        <span>{date ? `${label} ${date}${qualifier}` : "Subscription date unavailable"}</span>
        <span className="flex shrink-0 items-center gap-1 text-[10px]">
          Billing <span aria-hidden="true" className="inline-block transition-transform group-open:rotate-90">›</span>
        </span>
      </summary>
      <div className="mt-2 border-t border-[var(--hair)] pt-2 text-[10px]">
        {checked ? <p className="m-0">Last checked {checked}</p> : null}
        <p className="m-0">{metadata?.source === "localToken" ? "Cached from Codex · renewal status unknown" : metadata?.source === "billing" ? "Source: ChatGPT billing" : "Connect billing to check your subscription date."}</p>
        {reading.unavailable ? <p className="mt-1 mb-0">Billing could not be checked. Sign in to the matching ChatGPT account and retry.</p> : null}
        {children}
      </div>
    </details>
  );
}

export function CodexSubscription({ accountId, current, now = Date.now(), header }: { accountId: string; current: boolean; now?: number; header?: (status: ReactNode) => ReactNode }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const query = useQuery({
    queryKey: ["subscription", "codex", accountId],
    queryFn: () => api.readCodexSubscription(accountId),
    staleTime: 24 * 60 * 60_000,
    gcTime: 24 * 60 * 60_000,
    refetchInterval: current ? 24 * 60 * 60_000 : false,
    refetchIntervalInBackground: false,
  });
  useSharedRead("subscription:codex");
  async function connect() {
    setBusy(true); setError(null);
    try { await api.connectCodexBilling(accountId); }
    catch (error) { setError(parseInvokeError(error).message); }
    finally { setBusy(false); }
  }
  const reading = query.data ?? { metadata: null, connected: false, unavailable: false };
  return (
    <>
    {header?.(<SubscriptionStatus reading={reading} now={now} />)}
    <div className="border-b border-[var(--hair)] px-3.5 py-2.5" aria-label="Codex subscription date">
      <SubscriptionDate reading={reading} now={now}>
      {current && query.data?.browserSupported !== false ? (
        <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-[var(--mute)]">
          <button type="button" disabled={busy} onClick={() => void connect()} className="underline underline-offset-2 disabled:opacity-50">
            {busy ? "Reading browser session…" : "Use browser session"}
          </button>
          <span>Reads ChatGPT cookies · may prompt for Keychain access</span>
        </div>
      ) : null}
      {current && query.data?.browserSupported === false ? <p className="mt-1">Browser billing import is available on macOS only.</p> : null}
      </SubscriptionDate>
      {error || query.isError ? <p role="alert" className="mt-1 text-[11px] text-[var(--trip)]">{error ?? "Could not read the saved subscription date. Retry shortly."}</p> : null}
    </div>
    </>
  );
}
