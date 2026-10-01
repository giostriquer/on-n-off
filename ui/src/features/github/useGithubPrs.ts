import { useRef } from "react";
import { keepPreviousData, queryOptions, useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
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
  const forceRef = useRef(false);
  const query = useQuery(
    githubQueryOptions(pollSeconds, () => {
      const force = forceRef.current;
      forceRef.current = false;
      return api.readGithubPrs(force);
    }),
  );
  useSharedRead("github:prs");
  function refresh() {
    forceRef.current = true;
    void query.refetch();
  }
  return { query, loading: query.isFetching, now: Date.now(), refresh };
}
