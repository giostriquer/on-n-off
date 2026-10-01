import type { AccountFooterState } from "@/features/accounts/AccountCardActions";
import type { ProviderLimits } from "$lib/limitsTypes";
import { AutomaticSpend } from "./AutomaticSpend";
import { UseBankedReset } from "./BankedResets";

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
      <AutomaticSpend accountId={entry.account.id} now={now} />
    </>
  );
}
