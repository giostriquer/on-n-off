import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { useSharedRead } from "$lib/useSharedRead";

export function useCodexSubscription(accountId: string, refresh = true) {
  useSharedRead("subscription:codex");
  return useQuery({
    queryKey: ["subscription", "codex", accountId],
    queryFn: () => api.readCodexSubscription(accountId),
    staleTime: 24 * 60 * 60_000,
    refetchOnMount: "always",
    gcTime: 24 * 60 * 60_000,
    refetchInterval: refresh ? 24 * 60 * 60_000 : false,
    refetchIntervalInBackground: false,
  });
}
