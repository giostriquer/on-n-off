import { keepPreviousData, queryOptions, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";
import { readForced } from "./forcedRead";

const GITHUB_PRS = ["github", "prs"] as const;
const STALE_MS = 15_000;

export const githubPrsAnnouncements = {
  "github:prs": [GITHUB_PRS],
} satisfies Record<Extract<SharedReadSource, "github:prs">, readonly QueryKey[]>;

export function githubPrsQuery(pollSeconds: number) {
  return queryOptions({
    queryKey: GITHUB_PRS,
    queryFn: () => api.readGithubPrs(false),
    staleTime: STALE_MS,
    refetchInterval: pollSeconds * 1000,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    placeholderData: keepPreviousData,
  });
}

export async function refreshGithubPrs(client: QueryClient): Promise<void> {
  await readForced(client, GITHUB_PRS, (force) => api.readGithubPrs(force)).catch(() => undefined);
}
