import { useState } from "react";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { useCodexSubscription } from "$lib/useCodexSubscription";
import { SubscriptionDate } from "@/features/limits/SubscriptionDate";

export function AccountBilling({ accountId, disabled = false, showDate = true }: { accountId: string; disabled?: boolean; showDate?: boolean }) {
  const query = useCodexSubscription(accountId);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reading = query.data;
  const needsConnection = reading && (reading.metadata?.source !== "billing" || reading.unavailable || reading.metadata.stale);
  const showConnect = needsConnection && reading.browserSupported && reading.canConnect;
  if (!showDate && !showConnect && !error && !query.isError && !busy) return null;
  async function connect() {
    setBusy(true); setError(null);
    try { await api.connectCodexBilling(accountId); await query.refetch(); }
    catch (error) { setError(parseInvokeError(error).message); }
    finally { setBusy(false); }
  }
  return <>
      {showDate && reading && <div className="basis-full"><SubscriptionDate reading={reading} /></div>}
      {showConnect && <button type="button" disabled={disabled || busy} onClick={() => void connect()}
        title="Read the matching ChatGPT browser session; may request Keychain access."
        className="rounded-md border border-[var(--hair)] px-2.5 py-1.5 text-[11px] hover:bg-[var(--wash)] disabled:opacity-45">
        {busy ? "Checking billing…" : error || reading.unavailable ? "Retry billing" : "Connect billing"}
      </button>}
    {(error || query.isError) && <p role="alert" className="m-0 min-w-0 basis-full break-words text-[11px] text-[var(--trip)]">{error ?? "Could not read subscription details."}</p>}
  </>;
}
