import { AccountBilling } from "@/features/accounts/AccountBilling";
import type { AccountFooterState } from "@/features/accounts/AccountCardActions";
import type { ProviderLimits } from "$lib/limitsTypes";
import { UseBankedReset } from "./BankedResets";

/**
 * A Codex card's extra account actions. Spending a banked reset waits for the native login to be
 * confirmed as this account, because Codex spends it on whoever is signed in; billing only reads a
 * browser session for the card's saved identity, so it follows the account controls alone.
 */
export function CodexAccountActions({ entry, label, now, state }: {
  entry: ProviderLimits;
  label: string;
  now: number;
  state: AccountFooterState;
}) {
  if (!entry.account) return null;
  return (
    <>
      <UseBankedReset entry={entry} label={label} current={state.current} now={now} disabled={state.blocked || state.unconfirmedCurrent} />
      <AccountBilling accountId={entry.account.id} disabled={state.blocked} showDate={false} />
    </>
  );
}
