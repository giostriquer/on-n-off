/** Display formatting for the Pull requests screen (pure). */

import type {
  CiState,
  GithubPr,
  GithubPrList,
  GithubPrsData,
  GithubStatus,
  MergeKind,
  ReviewDecision,
} from "./githubTypes";

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

/** Red needs someone now, amber is waiting; green and grey are calm, ready included. */
const TONE_RANK: Record<CiTone, number> = { trip: 0, warn: 1, live: 2, mute: 2 };

/** How much a row needs someone: the reddest tone among its CI glyph and badges. */
function attentionRank(pr: GithubPr): number {
  return Math.min(
    TONE_RANK[ciTone(pr.ci)],
    TONE_RANK[reviewBadge(pr.reviewDecision)?.tone ?? "mute"],
    TONE_RANK[mergeBadge(pr)?.tone ?? "mute"],
  );
}

/** What needs fixing first, then what is waiting, then everything else; newest activity first within a group. */
export function orderPrs(items: readonly GithubPr[]): GithubPr[] {
  return [...items].sort(
    (a, b) => attentionRank(a) - attentionRank(b) || Date.parse(b.updatedAt) - Date.parse(a.updatedAt),
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

/** One repository's rows; `repo` is GitHub's `owner/name`. */
export type PrGroup = { repo: string; items: GithubPr[] };

/**
 * Rows grouped by repository so a list from many repositories reads in sections and no row has
 * to repeat where it lives; repositories are alphabetical and each group keeps the order it was
 * given (call after `orderPrs`).
 */
export function groupPrsByRepo(items: readonly GithubPr[]): PrGroup[] {
  const groups = new Map<string, GithubPr[]>();
  for (const pr of items) {
    const group = groups.get(pr.repo);
    if (group) group.push(pr);
    else groups.set(pr.repo, [pr]);
  }
  return [...groups]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([repo, rows]) => ({ repo, items: rows }));
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

/** A small outlined word on a row: the review decision, the merge state. */
export type RowBadge = { label: string; tone: CiTone };

/** "Review required" is every open PR's default state, so only the other two earn a badge. */
export function reviewBadge(decision: ReviewDecision | null | undefined): RowBadge | null {
  switch (decision) {
    case "APPROVED":
      return { label: "Approved", tone: "live" };
    case "CHANGES_REQUESTED":
      return { label: "Changes requested", tone: "trip" };
    default:
      return null;
  }
}

/** One badge per merge kind; the classification itself is the backend's (`mergeKind` on the row). */
const MERGE_BADGES: Record<MergeKind, RowBadge> = {
  conflicts: { label: "Conflicts", tone: "trip" },
  queued: { label: "Queued", tone: "live" },
  autoMerge: { label: "Auto-merge", tone: "mute" },
  ready: { label: "Ready to merge", tone: "live" },
  behind: { label: "Behind base", tone: "warn" },
  blocked: { label: "Blocked", tone: "warn" },
};

/** The one merge-state badge a row shows; the queue badge carries the position when known. */
export function mergeBadge(pr: GithubPr): RowBadge | null {
  const kind = pr.mergeKind;
  if (!kind) return null;
  const badge = MERGE_BADGES[kind];
  const position = kind === "queued" ? pr.mergeQueue?.position : null;
  return position ? { ...badge, label: `Queued #${position}` } : badge;
}

/**
 * Case-insensitive substring match over what a row shows: number, title, repository, author,
 * both branches, its badges (draft, team, the review decision, the merge state) and its CI
 * state's label.
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
      reviewBadge(pr.reviewDecision)?.label ?? "",
      mergeBadge(pr)?.label ?? "",
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

export type PrsSummary = {
  mine: number;
  /** Red CI among the user's own pull requests that were loaded. */
  failing: number;
  /** Own pull requests (loaded) with merge conflicts. */
  conflicts: number;
  /** Own pull requests (loaded) with every merge requirement met and nothing merging them yet. */
  ready: number;
  /** True when GitHub holds more own pull requests than were loaded, so the counts are floors. */
  countsArePartial: boolean;
  review: number;
  assigned: number;
};

/** The header's one-line summary. */
export function prsSummary(data: GithubPrsData): PrsSummary {
  return {
    mine: data.mine.total,
    failing: data.mine.items.filter((pr) => pr.ci === "failure" || pr.ci === "error").length,
    conflicts: data.mine.items.filter((pr) => pr.mergeKind === "conflicts").length,
    ready: data.mine.items.filter((pr) => pr.mergeKind === "ready").length,
    countsArePartial: truncated(data.mine),
    review: data.reviewRequested.total,
    assigned: data.assigned.total,
  };
}
