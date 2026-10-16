import { queryOptions, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { AccountProvider, AccountsReading } from "$lib/accountTypes";
import type { SharedReadSource } from "$lib/types";
import { rereadLimits } from "./limits";

const ACCOUNT_LISTS = ["accounts"] as const;
const CODEX_SUBSCRIPTIONS = ["subscription", "codex"] as const;
const DAY_MS = 24 * 60 * 60_000;

function accountListKey(provider: AccountProvider) {
  return [...ACCOUNT_LISTS, provider] as const;
}

export const accountsAnnouncements = {
  accounts: [ACCOUNT_LISTS, CODEX_SUBSCRIPTIONS],
} satisfies Record<Extract<SharedReadSource, "accounts">, readonly QueryKey[]>;

export function accountListQuery(provider: AccountProvider) {
  return queryOptions({ queryKey: accountListKey(provider), queryFn: () => api.readAccounts(provider), retry: false });
}

export function accountPreferenceQuery() {
  return queryOptions({ queryKey: [...ACCOUNT_LISTS, "preferences"], queryFn: () => api.readAccountPreferences(), retry: false });
}

export function codexSubscriptionQuery(accountId: string, refresh = true) {
  return queryOptions({
    queryKey: [...CODEX_SUBSCRIPTIONS, accountId],
    queryFn: () => api.readCodexSubscription(accountId),
    staleTime: DAY_MS,
    refetchOnMount: "always",
    gcTime: DAY_MS,
    refetchInterval: refresh ? DAY_MS : false,
    refetchIntervalInBackground: false,
  });
}

export function cachedAccountList(client: QueryClient, provider: AccountProvider): AccountsReading | undefined {
  return client.getQueryData<AccountsReading>(accountListKey(provider));
}

export function updateAccountList(
  client: QueryClient,
  provider: AccountProvider,
  update: (reading: AccountsReading) => AccountsReading,
): void {
  client.setQueryData<AccountsReading>(accountListKey(provider), (reading) => reading && update(reading));
}

export function rereadAccountLists(client: QueryClient): Promise<void> {
  return client.invalidateQueries({ queryKey: ACCOUNT_LISTS });
}

export async function accountChanged(client: QueryClient, provider: AccountProvider): Promise<void> {
  await Promise.all([
    rereadAccountLists(client),
    rereadLimits(client, provider),
    client.invalidateQueries({ queryKey: CODEX_SUBSCRIPTIONS }),
  ]);
}
