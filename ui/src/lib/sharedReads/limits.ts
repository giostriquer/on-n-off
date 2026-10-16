import { keepPreviousData, queryOptions, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { ProviderLimits } from "$lib/limitsTypes";
import { LIMITS_PROVIDERS, type LimitsProvider } from "$lib/providers";
import type { LimitsPollMinutes, SharedReadSource } from "$lib/types";
import { readForced } from "./forcedRead";

const LIMITS = ["limits"] as const;

function limitsKey(provider: LimitsProvider) {
  return [...LIMITS, provider] as const;
}

export const limitsAnnouncements = {
  "limits:claude": [limitsKey("claude")],
  "limits:codex": [limitsKey("codex")],
  "limits:muse": [limitsKey("muse")],
} satisfies Record<Extract<SharedReadSource, `limits:${LimitsProvider}`>, readonly QueryKey[]>;

export const LIMITS_CHECK_MINUTES = 5;

function limitsRefreshMs(pollMinutes: LimitsPollMinutes): number {
  return Math.min(pollMinutes, LIMITS_CHECK_MINUTES) * 60_000;
}

export function limitsQuery(provider: LimitsProvider, pollMinutes: LimitsPollMinutes) {
  return queryOptions({
    queryKey: limitsKey(provider),
    queryFn: () => api.readLimits(provider, false),
    staleTime: limitsRefreshMs(pollMinutes),
    refetchInterval: limitsRefreshMs(pollMinutes),
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    placeholderData: keepPreviousData,
  });
}

export async function refreshLimits(client: QueryClient): Promise<void> {
  await Promise.allSettled(
    LIMITS_PROVIDERS.map((provider) => readForced(client, limitsKey(provider), (force) => api.readLimits(provider, force))),
  );
}

export async function refetchStaleLimits(client: QueryClient, pollMinutes: LimitsPollMinutes): Promise<void> {
  await client.refetchQueries({
    queryKey: LIMITS,
    type: "active",
    predicate: (query) =>
      query.state.dataUpdatedAt > 0 && Date.now() - query.state.dataUpdatedAt >= limitsRefreshMs(pollMinutes),
  });
}

export function rereadLimits(client: QueryClient, provider: LimitsProvider): Promise<void> {
  return client.invalidateQueries({ queryKey: limitsKey(provider) });
}

export function updateLimits(
  client: QueryClient,
  provider: LimitsProvider,
  update: (entries: ProviderLimits[]) => ProviderLimits[],
): void {
  client.setQueryData<ProviderLimits[]>(limitsKey(provider), (entries) => entries && update(entries));
}
