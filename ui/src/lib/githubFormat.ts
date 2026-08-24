/** Display formatting for the Pull requests screen (pure). */

import type { CiState, GithubPr, GithubPrList, GithubStatus, ReviewDecision } from "./githubTypes";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** The app's status tones: `--live` green, `--trip` red, `--warn` amber, `--mute` grey. */
export type CiTone = "live" | "trip" | "warn" | "mute";

export function ciTone(ci: CiState): CiTone {
  switch (ci) {
    case "success":
      return "live";
    case "failure":
    case "error":
      return "trip";
    case "pending":
      return "warn";
    case "none":
      return "mute";
  }
}

export function ciToneColor(tone: CiTone): string {
  return `var(--${tone})`;
}

export function ciLabel(ci: CiState): string {
  switch (ci) {
    case "success":
      return "CI passing";
    case "failure":
      return "CI failing";
    case "error":
      return "CI errored";
    case "pending":
      return "CI pending";
    case "none":
      return "No checks";
  }
}

function ciRank(ci: CiState): number {
  switch (ci) {
    case "failure":
    case "error":
      return 0;
    case "pending":
      return 1;
    case "success":
    case "none":
      return 2;
  }
}

/** Failing CI first, then pending, then everything else; newest activity first within a group. */
export function orderPrs(items: readonly GithubPr[]): GithubPr[] {
  return [...items].sort(
    (a, b) => ciRank(a.ci) - ciRank(b.ci) || Date.parse(b.updatedAt) - Date.parse(a.updatedAt),
  );
}

/** "just now" / "5m ago" / "2h ago" / "3d ago"; empty when the instant is unreadable. */
export function formatUpdatedAgo(iso: string | null | undefined, nowMs: number): string {
  const at = iso ? Date.parse(iso) : Number.NaN;
  if (Number.isNaN(at)) return "";
  const elapsed = Math.max(0, nowMs - at);
  if (elapsed < MINUTE_MS) return "just now";
  if (elapsed < HOUR_MS) return `${Math.floor(elapsed / MINUTE_MS)}m ago`;
  if (elapsed < DAY_MS) return `${Math.floor(elapsed / HOUR_MS)}h ago`;
  return `${Math.floor(elapsed / DAY_MS)}d ago`;
}

/** "3", or "50 of 137" when GitHub holds more than the page that was read. */
export function listCountLabel(list: GithubPrList): string {
  return list.total > list.items.length ? `${list.items.length} of ${list.total}` : String(list.items.length);
}

export function statusHeadline(status: GithubStatus): string {
  switch (status) {
    case "ok":
      return "";
    case "ghMissing":
      return "GitHub CLI not found";
    case "ghNotLoggedIn":
      return "Not signed in to GitHub";
    case "tokenRejected":
      return "GitHub sign-in rejected";
    case "rateLimited":
      return "GitHub rate limit reached";
    case "network":
      return "GitHub unreachable";
  }
}

export function reviewDecisionLabel(decision: ReviewDecision | null | undefined): string {
  switch (decision) {
    case "APPROVED":
      return "Approved";
    case "CHANGES_REQUESTED":
      return "Changes requested";
    case "REVIEW_REQUIRED":
      return "Review required";
    default:
      return "";
  }
}
