import { useEffect, useState } from "react";
import { useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import type { ProviderLimits } from "$lib/limitsTypes";
import { limitsQuery, refreshLimits, type LimitsProvider } from "$lib/sharedReads/limits";
import type { LimitsPollMinutes } from "$lib/types";

type ProviderQuery = {
  provider: LimitsProvider;
  query: UseQueryResult<ProviderLimits[]>;
};

export function useLimitsProviders(pollMinutes: LimitsPollMinutes = 5) {
  const client = useQueryClient();
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const update = () => setNow(Date.now());
    const timer = window.setInterval(update, 60_000);
    window.addEventListener("focus", update);
    return () => { window.clearInterval(timer); window.removeEventListener("focus", update); };
  }, []);
  const providers: ProviderQuery[] = [
    { provider: "claude", query: useQuery(limitsQuery("claude", pollMinutes)) },
    { provider: "codex", query: useQuery(limitsQuery("codex", pollMinutes)) },
    { provider: "muse", query: useQuery(limitsQuery("muse", pollMinutes)) },
  ];
  const loading = providers.some(({ query }) => query.isFetching);

  function refresh() {
    void refreshLimits(client);
  }

  return { providers, loading, now, refresh };
}
