import { statusHeadline } from "$lib/githubFormat";
import type { GithubPrs } from "$lib/githubTypes";

export type PrBanner = { headline: string; hint: string; tone: "trip" | "warn" };

export function prBanner(prs: GithubPrs): PrBanner | null {
  const standing = prs.standing;
  if (!standing || standing.kind === "live") return null;
  return {
    headline: statusHeadline(prs.status),
    hint: standing.message.text,
    tone: standing.message.tone === "error" ? "trip" : "warn",
  };
}

export function showsLastRead(prs: GithubPrs): boolean {
  return prs.standing?.kind === "lastObserved";
}
