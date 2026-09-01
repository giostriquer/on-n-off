import { useRef } from "react";
import { keepPreviousData, useQuery, type UseQueryResult } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import type { LimitsPollMinutes } from "$lib/types";

export function limitsRefreshMs(pollMinutes: LimitsPollMinutes): number {
  return pollMinutes * 60_000;
}

export type ProviderQuery = {
  provider: AgentId;
  query: UseQueryResult<ProviderLimits[]>;
  refresh: () => void;
};

/**
 * One query per provider. The backend answers with the signed-in account first (any status)
 * followed by remembered snapshots of other accounts. Only `refresh()` (the button) sets `force`,
 * which makes the backend re-read the macOS Keychain instead of its in-process memo; background
 * refetches never do, so an "Allow"-once user is not re-prompted on every focus.
 */
function useProviderLimits(provider: AgentId, pollMinutes: LimitsPollMinutes): ProviderQuery {
  const forceRef = useRef(false);
  const query = useQuery({
    queryKey: ["limits", provider],
    queryFn: () => {
      const force = forceRef.current;
      forceRef.current = false;
      return api.readLimits(provider, force);
    },
    staleTime: limitsRefreshMs(pollMinutes),
    refetchInterval: limitsRefreshMs(pollMinutes),
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    placeholderData: keepPreviousData,
  });
  function refresh() {
    forceRef.current = true;
    void query.refetch();
  }
  return { provider, query, refresh };
}

/** Canonical Claude + Codex query state shared by the full screen and menu-bar surface. */
export function useLimitsProviders(pollMinutes: LimitsPollMinutes = 5) {
  const providers = [
    useProviderLimits("claude", pollMinutes),
    useProviderLimits("codex", pollMinutes),
  ];
  const loading = providers.some(({ query }) => query.isFetching);

  function refresh() {
    for (const provider of providers) {
      provider.refresh();
    }
  }

  return { providers, loading, now: Date.now(), refresh };
}
