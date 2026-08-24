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
