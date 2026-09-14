import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery, type QueryClient, type UseQueryResult } from "@tanstack/react-query";
import * as api from "$lib/api";
import { useSharedRead } from "$lib/useSharedRead";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import type { LimitsPollMinutes } from "$lib/types";

/** The providers the backend caches process-wide, and so can announce; see `read_revision.rs`. */
type SharedLimitsProvider = Extract<AgentId, "claude" | "codex">;

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
 *
 * That backend memo is shared with the menu-bar popover, the limits monitor and the notch, so
 * any of them may be the one that fetches; `useSharedRead` is how this query hears about it.
 */
function useProviderLimits(
  provider: SharedLimitsProvider,
  pollMinutes: LimitsPollMinutes,
): ProviderQuery {
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
  useSharedRead(`limits:${provider}`);
  function refresh() {
    forceRef.current = true;
    void query.refetch();
  }
  return { provider, query, refresh };
}

/** Canonical Claude + Codex query state shared by the full screen and menu-bar surface. */
export function useLimitsProviders(pollMinutes: LimitsPollMinutes = 5) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const update = () => setNow(Date.now());
    const timer = window.setInterval(update, 60_000);
    window.addEventListener("focus", update);
    return () => { window.clearInterval(timer); window.removeEventListener("focus", update); };
  }, []);
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

  return { providers, loading, now, refresh };
}

/** The shell refresh uses the same query keys and backend cache as every Limits surface. */
export async function refreshLimits(client: QueryClient) {
  await Promise.allSettled((["claude", "codex"] as const).map(async provider => {
    const queryKey = ["limits", provider];
    // Joining a background read would discard the user's explicit force request.
    let existing = client.getQueryCache().find({ queryKey, exact: true });
    while (existing?.state.fetchStatus === "fetching" && existing.promise) {
      await existing.promise.catch(() => {});
      // Shared-read invalidation can cancel and replace the request we just awaited.
      existing = client.getQueryCache().find({ queryKey, exact: true });
    }
    let forceNextRead = true;
    return client.fetchQuery({
      queryKey, staleTime: 0,
      queryFn: () => {
        const force = forceNextRead;
        forceNextRead = false;
        return api.readLimits(provider, force);
      },
    });
  }));
}
