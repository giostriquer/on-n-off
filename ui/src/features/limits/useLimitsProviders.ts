import { useRef } from "react";
import { keepPreviousData, useQuery, type UseQueryResult } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";

export const LIMITS_STALE_MS = 60_000;

/**
 * How long one provider's answer stays fresh. A current-account read that did not come back `ok` is worth
 * re-reading at the next chance: the CLIs renew their own access tokens, so such a failure heals
 * by itself and the screen should notice it without the user pressing refresh.
 */
export function limitsStaleMs(entries: ProviderLimits[] | undefined): number {
  const current = entries?.find((entry) => entry.currentAccount);
  return current && current.status !== "ok" ? 0 : LIMITS_STALE_MS;
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
function useProviderLimits(provider: AgentId): ProviderQuery {
  const forceRef = useRef(false);
  const query = useQuery({
    queryKey: ["limits", provider],
    queryFn: () => {
      const force = forceRef.current;
      forceRef.current = false;
      return api.readLimits(provider, force);
    },
    staleTime: (query) => limitsStaleMs(query.state.data),
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

  function refresh() {
    for (const provider of providers) {
      provider.refresh();
    }
  }

  return { providers, loading, now: Date.now(), refresh };
}
