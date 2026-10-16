import { useEffect, useState } from "react";
import { keepPreviousData, useQuery, useQueryClient, type QueryClient, type UseQueryResult } from "@tanstack/react-query";
import * as api from "$lib/api";
import { readForced } from "$lib/sharedReads/forcedRead";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import type { LimitsPollMinutes } from "$lib/types";

type SharedLimitsProvider = Extract<AgentId, "claude" | "codex" | "muse">;

export function limitsRefreshMs(pollMinutes: LimitsPollMinutes): number {
  // The shared cache decides which accounts are due, including active accounts near exhaustion.
  return Math.min(pollMinutes, 5) * 60_000;
}

export type ProviderQuery = {
  provider: AgentId;
  query: UseQueryResult<ProviderLimits[]>;
  refresh: () => void;
};

function useProviderLimits(
  provider: SharedLimitsProvider,
  pollMinutes: LimitsPollMinutes,
): ProviderQuery {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ["limits", provider],
    queryFn: () => api.readLimits(provider, false),
    staleTime: limitsRefreshMs(pollMinutes),
    refetchInterval: limitsRefreshMs(pollMinutes),
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    placeholderData: keepPreviousData,
  });
  function refresh() {
    void forceLimits(client, provider).catch(() => undefined);
  }
  return { provider, query, refresh };
}

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
    useProviderLimits("muse", pollMinutes),
  ];
  const loading = providers.some(({ query }) => query.isFetching);

  function refresh() {
    for (const provider of providers) {
      provider.refresh();
    }
  }

  return { providers, loading, now, refresh };
}

function forceLimits(client: QueryClient, provider: SharedLimitsProvider) {
  return readForced(client, ["limits", provider], (force) => api.readLimits(provider, force));
}

export async function refreshLimits(client: QueryClient) {
  await Promise.allSettled((["claude", "codex", "muse"] as const).map((provider) => forceLimits(client, provider)));
}
