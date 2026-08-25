export type GithubStatus =
  | "ok"
  | "ghMissing"
  | "ghNotLoggedIn"
  | "tokenRejected"
  | "rateLimited"
  | "network";

/** The head commit's status-check rollup; `none` also covers states this build does not know. */
export type CiState = "none" | "pending" | "success" | "failure" | "error";

export type ReviewDecision = "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED";

/** Whether the head merges into the base without conflicts; `unknown` while GitHub computes it. */
export type Mergeable = "mergeable" | "conflicting" | "unknown";

/**
 * GitHub's merge state, collapsed: `clean` has every requirement met, `blocked` is branch
 * protection (review, required checks), `behind` needs an update from the base, `dirty` is
 * conflicts, `unstable` a non-required check failing; `unknown` also covers states this build
 * does not know.
 */
export type MergeState = "clean" | "unstable" | "blocked" | "behind" | "dirty" | "draft" | "unknown";

export type GithubPr = {
  /** GitHub's node id, stable across pushes and renames. */
  id: string;
  number: number;
  title: string;
  url: string;
  /** `owner/name` */
  repo: string;
  author: string;
  isDraft: boolean;
  reviewDecision?: ReviewDecision | null;
  ci: CiState;
  headRef: string;
  baseRef: string;
  /** RFC 3339 */
  updatedAt: string;
  /** Review-requested list only: whether the request named the user or one of their teams. */
  reviewRequest?: "direct" | "team" | null;
  mergeable: Mergeable;
  mergeState: MergeState;
  /** Present while the pull request sits in a merge queue; `position` is 1-based when known. */
  mergeQueue?: { position?: number | null } | null;
  /** Auto-merge is on: GitHub merges once the requirements are met. */
  autoMerge: boolean;
};

export type GithubPrList = {
  /** Matches on GitHub; can exceed `items.length` (one page is read). */
  total: number;
  items: GithubPr[];
};

export type GithubRateLimit = {
  remaining: number;
  /** RFC 3339 */
  resetAt: string;
};

/** What one successful read produced (mirrors `GithubPrsData`, flattened into `GithubPrs`). */
export type GithubPrsData = {
  viewer?: string | null;
  /** RFC 3339 instant of the read that produced the lists (the snapshot's, when stale). */
  fetchedAt?: string | null;
  /** The scope qualifiers applied to `mine`. */
  scope: string[];
  mine: GithubPrList;
  reviewRequested: GithubPrList;
  assigned: GithubPrList;
  rateLimit?: GithubRateLimit | null;
};

/** The three lists, in the order the screen shows them. */
export type GithubListId = keyof Pick<GithubPrsData, "mine" | "reviewRequested" | "assigned">;
export const GITHUB_LIST_IDS: readonly GithubListId[] = ["mine", "reviewRequested", "assigned"];

/**
 * Mirrors `GithubPrsDto`: GitHub-side problems arrive as a status plus a hint, not an error.
 * `stale: true` means the lists come from the last successful read on disk.
 */
export type GithubPrs = GithubPrsData & {
  status: GithubStatus;
  hint?: string | null;
  stale: boolean;
  warnings?: string[];
};
