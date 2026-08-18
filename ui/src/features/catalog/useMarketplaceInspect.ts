import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { GithubRepo } from "$lib/installSource";

const MARKETPLACE_STALE_MS = 5 * 60_000;

/** Downloads and reads a GitHub marketplace once per repo/ref; `null` repo disables the query. */
export function useMarketplaceInspect(repo: GithubRepo | null) {
  return useQuery({
    queryKey: ["marketplace", repo?.owner ?? "", repo?.repo ?? "", repo?.ref ?? ""],
    queryFn: () => api.inspectMarketplace(repo!.owner, repo!.repo, repo!.ref),
    enabled: repo !== null,
    staleTime: MARKETPLACE_STALE_MS,
    retry: false,
  });
}
