import { queryOptions, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { AccountsReading } from "$lib/accountTypes";
import type { AccountProvider } from "$lib/providers";
import type { SharedReadSource } from "$lib/types";
import { rereadLimits } from "./limits";

const ACCOUNTS = ["accounts"] as const;
const CODEX_SUBSCRIPTIONS = ["subscription", "codex"] as const;
const ANNOUNCED = [ACCOUNTS, CODEX_SUBSCRIPTIONS] as const;
const DAY_MS = 24 * 60 * 60_000;

function accountListKey(provider: AccountProvider) {
  return [...ACCOUNTS, provider] as const;
}

export const accountsAnnouncements = {
  accounts: ANNOUNCED,
} satisfies Record<Extract<SharedReadSource, "accounts">, readonly QueryKey[]>;

export function accountListQuery(provider: AccountProvider) {
  return queryOptions({ queryKey: accountListKey(provider), queryFn: () => api.readAccounts(provider), retry: false });
}

export function accountPreferenceQuery() {
  return queryOptions({ queryKey: [...ACCOUNTS, "preferences"], queryFn: () => api.readAccountPreferences(), retry: false });
}

export function codexSubscriptionQuery(accountId: string, poll = true) {
  return queryOptions({
    queryKey: [...CODEX_SUBSCRIPTIONS, accountId],
    queryFn: () => api.readCodexSubscription(accountId),
    staleTime: DAY_MS,
    refetchOnMount: "always",
    gcTime: DAY_MS,
    refetchInterval: poll ? DAY_MS : false,
    refetchIntervalInBackground: false,
  });
}

export function cachedAccountList(client: QueryClient, provider: AccountProvider): AccountsReading | undefined {
  return client.getQueryData<AccountsReading>(accountListKey(provider));
}

export function markProfilesArchived(
  client: QueryClient,
  provider: AccountProvider,
  observationIds: readonly string[],
  archived: boolean,
): void {
  client.setQueryData<AccountsReading>(accountListKey(provider), (reading) => reading && {
    ...reading,
    profiles: reading.profiles.map((profile) =>
      observationIds.includes(profile.observationId) ? { ...profile, archived } : profile),
  });
}

export function rereadAccountListsAndPreference(client: QueryClient): Promise<void> {
  return client.invalidateQueries({ queryKey: ACCOUNTS });
}

export async function accountChanged(client: QueryClient, provider: AccountProvider): Promise<void> {
  await Promise.all([
    ...ANNOUNCED.map((queryKey) => client.invalidateQueries({ queryKey })),
    rereadLimits(client, provider),
  ]);
}
