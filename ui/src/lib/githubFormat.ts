import type {
  CiState,
  GithubPr,
  GithubPrList,
  GithubStatus,
  MergeKind,
  ReviewDecision,
} from "./githubTypes";

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

const TONE_RANK: Record<CiTone, number> = { trip: 0, warn: 1, live: 2, mute: 2 };

function attentionRank(pr: GithubPr): number {
  return Math.min(
    TONE_RANK[ciTone(pr.ci)],
    TONE_RANK[reviewBadge(pr.reviewDecision)?.tone ?? "mute"],
    TONE_RANK[mergeBadge(pr)?.tone ?? "mute"],
  );
}

export function orderPrs(items: readonly GithubPr[]): GithubPr[] {
  return [...items].sort(
    (a, b) => attentionRank(a) - attentionRank(b) || Date.parse(b.updatedAt) - Date.parse(a.updatedAt),
  );
}

export type PrGroup = { repo: string; items: GithubPr[] };

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

export function listCountLabel(list: GithubPrList, matches?: number): string {
  if (matches !== undefined) {
    return `${matches} of ${list.items.length}${truncated(list) ? " loaded" : ""}`;
  }
  return truncated(list) ? `${list.items.length} of ${list.total}` : String(list.items.length);
}

export type RowBadge = { label: string; tone: CiTone };

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

const MERGE_BADGES: Record<MergeKind, RowBadge> = {
  conflicts: { label: "Conflicts", tone: "trip" },
  queued: { label: "Queued", tone: "live" },
  autoMerge: { label: "Auto-merge", tone: "mute" },
  ready: { label: "Ready to merge", tone: "live" },
  behind: { label: "Behind base", tone: "warn" },
  blocked: { label: "Blocked", tone: "warn" },
};

export function mergeBadge(pr: GithubPr): RowBadge | null {
  const kind = pr.mergeKind;
  if (!kind) return null;
  const badge = MERGE_BADGES[kind];
  const position = kind === "queued" ? pr.mergeQueue?.position : null;
  return position ? { ...badge, label: `Queued #${position}` } : badge;
}

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

