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

/**
 * What a row can say about merging, most pressing first, classified once by the backend
 * (`src-tauri/src/github/merge.rs`) from GitHub's mergeability, merge state, merge queue and
 * auto-merge: `blocked` is reserved for a block the row does not otherwise explain.
 */
export type MergeKind = "conflicts" | "queued" | "autoMerge" | "ready" | "behind" | "blocked";

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
  /**
   * The backend's verdict on merging; absent when there is nothing to say. The DTO also carries
   * the raw fields it was read from (`mergeable`, `mergeState`, `autoMerge`), which the screen
   * does not consult.
   */
  mergeKind?: MergeKind | null;
  /** Present while the pull request sits in a merge queue; `position` is 1-based when known. */
  mergeQueue?: { position?: number | null } | null;
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
