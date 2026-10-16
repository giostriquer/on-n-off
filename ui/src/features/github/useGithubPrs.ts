import { keepPreviousData, queryOptions, useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { readForced } from "$lib/sharedReads/forcedRead";
import { useSharedRead } from "$lib/useSharedRead";
import type { GithubPrs } from "$lib/githubTypes";

const STALE_MS = 15_000;

export function githubQueryOptions(pollSeconds: number, read: () => Promise<GithubPrs>) {
  return queryOptions({
    queryKey: ["github", "prs"] as const,
    queryFn: read,
    staleTime: STALE_MS,
    refetchInterval: pollSeconds * 1000,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    placeholderData: keepPreviousData,
  });
}

export function useGithubPrs(pollSeconds: number) {
  const client = useQueryClient();
  const query = useQuery(githubQueryOptions(pollSeconds, () => api.readGithubPrs(false)));
  useSharedRead("github:prs");
  function refresh() {
    void readForced(client, ["github", "prs"], (force) => api.readGithubPrs(force)).catch(() => undefined);
  }
  return { query, loading: query.isFetching, now: Date.now(), refresh };
}
