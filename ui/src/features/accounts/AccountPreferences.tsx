import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CardToggle, SettingRow, SettingsCard, SwitchRow } from "@/components/SettingsCard";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { useSharedRead } from "$lib/useSharedRead";

const LABEL = "Automatically save accounts I sign in to";

export function AccountPreferences() {
  const saving = useAutomaticAccountSaving();
  return (
    <SettingsCard
      title="Accounts"
      description="Saves each account you sign in to, so Limits can switch back to it."
      control={<CardToggle caption="Auto-save" on={saving.on} disabled={saving.locked} ariaLabel={LABEL} onToggle={saving.toggle} />}
    >
      {saving.problem && <SettingRow><p role="alert" className="m-0 text-[12px] text-[var(--trip)]">{saving.problem}</p></SettingRow>}
    </SettingsCard>
  );
}

/** The same switch as a row of the Add account menu. */
export function AutomaticAccountSaving() {
  const saving = useAutomaticAccountSaving();
  return <>
    <SwitchRow label={LABEL} on={saving.on} disabled={saving.locked} onToggle={saving.toggle} />
    {saving.problem && <p role="alert" className="text-[12px] text-[var(--trip)]">{saving.problem}</p>}
  </>;
}

function useAutomaticAccountSaving() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["accounts", "preferences"], queryFn: api.readAccountPreferences, retry: false });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useSharedRead("accounts");
  const on = query.data ?? false;
  async function change(enabled: boolean) {
    setBusy(true); setError(null);
    try { await api.accountAction("codex", enabled ? "remember" : "stopRemembering"); await client.invalidateQueries({ queryKey: ["accounts"] }); }
    catch (error) { setError(parseInvokeError(error).message); }
    finally { setBusy(false); }
  }
  return {
    on,
    locked: busy || query.isPending || query.isError,
    toggle: () => void change(!on),
    problem: error ?? (query.error ? parseInvokeError(query.error).message : null),
  };
}
