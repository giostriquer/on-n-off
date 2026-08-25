/** Display formatting for the Pull requests screen (pure). */

import type {
  CiState,
  GithubPr,
  GithubPrList,
  GithubPrsData,
  GithubStatus,
  ReviewDecision,
} from "./githubTypes";

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

function truncated(list: GithubPrList): boolean {
  return list.total > list.items.length;
}

/**
 * "3", or "50 of 137" when GitHub holds more than the page that was read. With a search on, the
 * denominator is what was actually searched — the loaded page — and says so when that is not
 * everything: "2 of 3", or "2 of 50 loaded".
 */
export function listCountLabel(list: GithubPrList, matches?: number): string {
  if (matches !== undefined) {
    return `${matches} of ${list.items.length}${truncated(list) ? " loaded" : ""}`;
  }
  return truncated(list) ? `${list.items.length} of ${list.total}` : String(list.items.length);
}

/**
 * Case-insensitive substring match over what a row shows: number, title, repository, author,
 * both branches, its badges (draft, team, the review decision) and its CI state's label.
 */
export function filterPrs(items: readonly GithubPr[], query: string): GithubPr[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...items];
  return items.filter((pr) =>
    [
      `#${pr.number}`,
      pr.title,
      pr.repo,
      pr.author,
      pr.headRef,
      pr.baseRef,
      pr.isDraft ? "draft" : "",
      pr.reviewRequest ?? "",
      reviewDecisionLabel(pr.reviewDecision),
      ciLabel(pr.ci),
    ].some((field) => field.toLowerCase().includes(needle)),
  );
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

/** Approved reads like passing CI, changes requested like failing CI; a pending review is quiet. */
export function reviewDecisionTone(decision: ReviewDecision | null | undefined): CiTone {
  switch (decision) {
    case "APPROVED":
      return "live";
    case "CHANGES_REQUESTED":
      return "trip";
    default:
      return "mute";
  }
}

/** "Review required" is every open PR's default state, so only the other two earn a badge. */
export function reviewDecisionLabel(decision: ReviewDecision | null | undefined): string {
  switch (decision) {
    case "APPROVED":
      return "Approved";
    case "CHANGES_REQUESTED":
      return "Changes requested";
    default:
      return "";
  }
}

export type PrsSummary = {
  mine: number;
  /** Red CI among the user's own pull requests that were loaded. */
  failing: number;
  /** True when GitHub holds more own pull requests than were loaded, so `failing` is a floor. */
  failingIsPartial: boolean;
  review: number;
  assigned: number;
};

/** The header's one-line summary. */
export function prsSummary(data: GithubPrsData): PrsSummary {
  return {
    mine: data.mine.total,
    failing: data.mine.items.filter((pr) => pr.ci === "failure" || pr.ci === "error").length,
    failingIsPartial: truncated(data.mine),
    review: data.reviewRequested.total,
    assigned: data.assigned.total,
  };
}
