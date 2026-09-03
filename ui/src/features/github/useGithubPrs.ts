import { useRef } from "react";
import { keepPreviousData, queryOptions, useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { useSharedRead } from "$lib/useSharedRead";
import type { GithubPrs } from "$lib/githubTypes";

/** Focus-driven refetches within this window are answered by the backend's own memory anyway. */
const STALE_MS = 15_000;

/**
 * Polls on the configured interval while the window is visible; a hidden window stops polling
 * so an idle laptop is not spending GitHub's rate-limit budget on a screen nobody is looking at.
 */
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

/**
 * Only `refresh()` (the button) sets `force`, which makes the backend skip its in-memory result;
 * interval and focus refetches never do, so they share one request with the CI monitor. That
 * shared result is also read by the notch's pull-request cell, so any of the three may be the
 * one that fetches; `useSharedRead` is how this query hears about it.
 */
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
