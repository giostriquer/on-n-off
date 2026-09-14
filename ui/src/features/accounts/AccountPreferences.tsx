import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { useSharedRead } from "$lib/useSharedRead";

export function AccountPreferences() {
  return <section aria-label="Accounts" className="rounded-lg border border-[var(--hair)] p-4">
    <h3 className="mt-0 text-[13px] font-semibold">Accounts</h3>
    <AutomaticAccountSaving />
  </section>;
}

export function AutomaticAccountSaving() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["accounts", "preferences"], queryFn: api.readAccountPreferences, retry: false });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useSharedRead("accounts");
  async function change(enabled: boolean) {
    setBusy(true); setError(null);
    try { await api.accountAction("codex", enabled ? "remember" : "stopRemembering"); await client.invalidateQueries({ queryKey: ["accounts"] }); }
    catch (error) { setError(parseInvokeError(error).message); }
    finally { setBusy(false); }
  }
  return <>
    <label className="flex items-center gap-2 text-[12px]"><input className="m-0 size-3.5 shrink-0" type="checkbox" checked={query.data ?? false} disabled={busy || query.isPending || query.isError} onChange={event => void change(event.target.checked)} />Automatically save accounts I sign in to</label>
    {(error || query.error) && <p role="alert" className="text-[12px] text-[var(--trip)]">{error ?? parseInvokeError(query.error).message}</p>}
  </>;
}
