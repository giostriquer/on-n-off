import { useRef } from "react";
import { keepPreviousData, useQuery, type UseQueryResult } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";

export const LIMITS_STALE_MS = 60_000;

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
function useProviderLimits(provider: AgentId): ProviderQuery {
  const forceRef = useRef(false);
  const query = useQuery({
    queryKey: ["limits", provider],
    queryFn: () => {
      const force = forceRef.current;
      forceRef.current = false;
      return api.readLimits(provider, force);
    },
    staleTime: LIMITS_STALE_MS,
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
export function useLimitsProviders() {
  const providers = [useProviderLimits("claude"), useProviderLimits("codex")];
  const loading = providers.some(({ query }) => query.isFetching);
  const asOf = providers
    .map(({ query }) => query.data?.find((entry) => entry.live && entry.status === "ok")?.fetchedAt ?? "")
    .filter(Boolean)
    .sort()
    .at(-1);

  function refresh() {
    for (const provider of providers) {
      provider.refresh();
    }
  }

  return { providers, loading, asOf, now: Date.now(), refresh };
}
