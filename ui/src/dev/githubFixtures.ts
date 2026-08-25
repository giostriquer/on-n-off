import type { CiState, GithubPr, GithubPrs, GithubStatus, MergeState, Mergeable, ReviewDecision } from "$lib/githubTypes";

/** Synthetic pull requests for the UI harness; nothing here is a real repository or person. */

const NOW = Date.parse("2026-08-24T20:00:00Z");

type Seed = {
  n: number;
  title: string;
  repo?: string;
  author?: string;
  ci?: CiState;
  draft?: boolean;
  decision?: ReviewDecision;
  team?: boolean;
  minutesAgo?: number;
  mergeable?: Mergeable;
  mergeState?: MergeState;
  /** In the merge queue at this position (0 for "queued, position unknown"). */
  queued?: number;
  autoMerge?: boolean;
};

function pr(seed: Seed, kind: "mine" | "review" | "assigned"): GithubPr {
  const repo = seed.repo ?? "acme/web";
  return {
    id: `PR_${kind}_${seed.n}`,
    number: seed.n,
    title: seed.title,
    url: `https://github.com/${repo}/pull/${seed.n}`,
    repo,
    author: seed.author ?? (kind === "mine" ? "you" : "mara"),
    isDraft: seed.draft ?? false,
    reviewDecision: seed.decision ?? "REVIEW_REQUIRED",
    ci: seed.ci ?? "success",
    headRef: `feat/${seed.title.toLowerCase().replace(/[^a-z0-9]+/g, "-").slice(0, 28)}`,
    baseRef: "main",
    updatedAt: new Date(NOW - (seed.minutesAgo ?? 5) * 60_000).toISOString(),
    ...(kind === "review" ? { reviewRequest: seed.team ? "team" : "direct" } : {}),
    mergeable: seed.mergeable ?? "mergeable",
    mergeState: seed.mergeState ?? (seed.draft ? "draft" : "blocked"),
    ...(seed.queued !== undefined ? { mergeQueue: { position: seed.queued || null } } : {}),
    autoMerge: seed.autoMerge ?? false,
  };
}

const MINE: Seed[] = [
  { n: 412, title: "Add retry with jitter to the sync worker", ci: "failure", minutesAgo: 3 },
  { n: 398, title: "Tighten the CSP for the updater window", ci: "success", decision: "APPROVED", mergeState: "clean", minutesAgo: 41 },
  { n: 401, title: "Migrate the settings store to versioned JSON", ci: "pending", draft: true, minutesAgo: 12 },
  { n: 388, title: "Rename the tray menu entries", ci: "success", mergeable: "conflicting", mergeState: "dirty", minutesAgo: 900 },
];

const REVIEW: Seed[] = [
  { n: 530, title: "Seal the export authority behind a capability port", author: "mara", ci: "failure", minutesAgo: 8 },
  { n: 529, title: "Enforce same-origin on the backend boundary", author: "mara", ci: "pending", minutesAgo: 2 },
  { n: 527, title: "Prove semantic list scale behaviour at 10k rows", author: "devon", ci: "success", team: true, decision: "APPROVED", queued: 2, mergeState: "clean", minutesAgo: 66 },
  { n: 91, title: "api: reject unsigned webhooks", repo: "acme/api", author: "lin", ci: "error", decision: "CHANGES_REQUESTED", minutesAgo: 190 },
  { n: 88, title: "api: paginate the audit log", repo: "acme/api", author: "lin", ci: "success", autoMerge: true, minutesAgo: 1440 },
  { n: 305, title: "tools: a much longer title than usual so truncation has something to do in a narrow window", repo: "octo/tools", author: "sam", ci: "pending", team: true, minutesAgo: 30 },
  { n: 302, title: "tools: bump lockfile", repo: "octo/tools", author: "sam", ci: "none", minutesAgo: 2880 },
  { n: 525, title: "Content-addressed state activation", author: "devon", ci: "success", decision: "APPROVED", mergeState: "blocked", minutesAgo: 15 },
  { n: 521, title: "Qualify client state ownership", author: "mara", ci: "pending", minutesAgo: 9 },
  { n: 519, title: "Production QC execution lane", author: "devon", ci: "success", mergeState: "behind", minutesAgo: 75 },
  { n: 518, title: "Frozen dependency authority", author: "mara", ci: "failure", draft: true, minutesAgo: 120 },
  { n: 517, title: "Advisory artifact quality reviewer", author: "devon", ci: "success", minutesAgo: 4 },
];

const ASSIGNED: Seed[] = [{ n: 77, title: "Triage: flaky updater smoke on Windows", repo: "acme/web", author: "lin", ci: "success", minutesAgo: 500 }];

export function okPrs(overrides: Partial<GithubPrs> = {}): GithubPrs {
  return {
    status: "ok",
    stale: false,
    viewer: "you",
    fetchedAt: new Date(NOW - 20_000).toISOString(),
    scope: ["org:acme", "repo:octo/tools"],
    mine: { total: MINE.length, items: MINE.map((seed) => pr(seed, "mine")) },
    reviewRequested: { total: REVIEW.length, items: REVIEW.map((seed) => pr(seed, "review")) },
    assigned: { total: ASSIGNED.length, items: ASSIGNED.map((seed) => pr(seed, "assigned")) },
    rateLimit: { remaining: 4877, resetAt: new Date(NOW + 1_800_000).toISOString() },
    ...overrides,
  };
}

export function problemPrs(status: GithubStatus, hint: string): GithubPrs {
  return {
    status,
    hint,
    stale: false,
    scope: [],
    mine: { total: 0, items: [] },
    reviewRequested: { total: 0, items: [] },
    assigned: { total: 0, items: [] },
  };
}

export function manyPrs(): GithubPrs {
  const items = Array.from({ length: 50 }, (_, index) =>
    pr(
      {
        n: 1000 + index,
        title: `Batch change ${index + 1}`,
        ci: (["success", "pending", "failure"] as const)[index % 3],
        mergeable: index % 7 === 0 ? "conflicting" : "mergeable",
        mergeState: index % 7 === 0 ? "dirty" : index % 3 === 0 ? "clean" : "blocked",
        ...(index % 11 === 3 ? { queued: Math.floor(index / 11) + 1 } : {}),
        minutesAgo: index * 7,
      },
      "mine",
    ),
  );
  return okPrs({ mine: { total: 137, items } });
}

export const SCENARIOS: Record<string, () => GithubPrs | Promise<GithubPrs>> = {
  ok: () => okPrs(),
  empty: () =>
    okPrs({
      scope: [],
      mine: { total: 0, items: [] },
      reviewRequested: { total: 0, items: [] },
      assigned: { total: 0, items: [] },
    }),
  many: manyPrs,
  stale: () => okPrs({ status: "network", hint: "Could not reach GitHub (network error: Dns).", stale: true }),
  ghMissing: () => problemPrs("ghMissing", "Install the GitHub CLI (`gh`), sign in with `gh auth login`, then refresh."),
  ghNotLoggedIn: () => problemPrs("ghNotLoggedIn", "Sign in with `gh auth login` in a terminal, then refresh."),
  tokenRejected: () => problemPrs("tokenRejected", "GitHub rejected the `gh` login — run `gh auth login` again, then refresh."),
  rateLimited: () => problemPrs("rateLimited", "GitHub rate limit reached — polling resumes in 5 min."),
  network: () => problemPrs("network", "Could not reach GitHub (network error: Dns)."),
  loading: () => new Promise<GithubPrs>(() => undefined),
};
