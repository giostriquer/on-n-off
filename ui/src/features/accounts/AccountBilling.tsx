import { useState } from "react";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { useCodexSubscription } from "$lib/useCodexSubscription";
import { accountButton } from "./AccountManager";

export function AccountBilling({ accountId, disabled = false }: { accountId: string; disabled?: boolean }) {
  const query = useCodexSubscription(accountId);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reading = query.data;
  const needsConnection = reading && (reading.metadata?.source !== "billing" || reading.unavailable || reading.metadata.stale);
  const showConnect = needsConnection && reading.browserSupported && reading.canConnect;
  if (!showConnect && !error && !query.isError && !busy) return null;
  async function connect() {
    setBusy(true); setError(null);
    try { await api.connectCodexBilling(accountId); await query.refetch(); }
    catch (error) { setError(parseInvokeError(error).message); }
    finally { setBusy(false); }
  }
  return <>
      {showConnect && <button type="button" disabled={disabled || busy} onClick={() => void connect()}
        title="Read the matching ChatGPT browser session; may request Keychain access."
        className={`${accountButton} border-transparent text-left`}>
        {busy ? "Checking billing…" : error || reading.unavailable ? "Retry billing" : "Connect billing"}
      </button>}
    {(error || query.isError) && <p role="alert" className="m-0 min-w-0 basis-full break-words text-[11px] text-[var(--trip)]">{error ?? "Could not read subscription details."}</p>}
  </>;
}
