import type { AccountFooterState } from "@/features/accounts/AccountCardActions";
import type { ProviderLimits } from "$lib/limitsTypes";
import { UseBankedReset } from "./BankedResets";

/**
 * A Codex card's extra account actions. Spending a banked reset waits for the native login to be
 * confirmed as this account, because Codex spends it on whoever is signed in.
 */
export function CodexAccountActions({ entry, label, now, state }: {
  entry: ProviderLimits;
  label: string;
  now: number;
  state: AccountFooterState;
}) {
  if (!entry.account) return null;
  return (
      <UseBankedReset entry={entry} label={label} current={state.current} now={now} disabled={state.blocked || state.unconfirmedCurrent} />
  );
}
