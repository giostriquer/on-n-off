export const GITHUB_STATUSES = ["ok", "ghMissing", "ghNotLoggedIn", "tokenRejected", "rateLimited", "network"] as const;

export type GithubStatus = (typeof GITHUB_STATUSES)[number];

export type PrStandingMessage = { text: string; tone: "error" | "warning" };

export type PrReadStanding =
  | { kind: "live" }
  | { kind: "lastObserved"; message: PrStandingMessage }
  | { kind: "unavailable"; message: PrStandingMessage };

export type CiState = "none" | "pending" | "success" | "failure" | "error";

export type ReviewDecision = "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED";

export type MergeKind = "conflicts" | "queued" | "autoMerge" | "ready" | "behind" | "blocked";

export type GithubPr = {
  id: string;
  number: number;
  title: string;
  url: string;
  repo: string;
  author: string;
  isDraft: boolean;
  reviewDecision?: ReviewDecision | null;
  ci: CiState;
  headRef: string;
  baseRef: string;
  updatedAt: string;
  reviewRequest?: "direct" | "team" | null;
  mergeKind?: MergeKind | null;
  mergeQueue?: { position?: number | null } | null;
  contributed?: boolean;
};

export type GithubPrList = {
  total: number;
  items: GithubPr[];
};

export type GithubRateLimit = {
  remaining: number;
  resetAt: string;
};

export type GithubPrsData = {
  viewer?: string | null;
  fetchedAt?: string | null;
  scope: string[];
  mine: GithubPrList;
  reviewRequested: GithubPrList;
  assigned: GithubPrList;
  merged: GithubPrList;
  rateLimit?: GithubRateLimit | null;
};

export type GithubListId = keyof Pick<GithubPrsData, "mine" | "reviewRequested" | "assigned">;
export const GITHUB_LIST_IDS: readonly GithubListId[] = ["mine", "reviewRequested", "assigned"];

export type GithubPrs = GithubPrsData & {
  status: GithubStatus;
  warnings?: string[];
  standing?: PrReadStanding;
};
