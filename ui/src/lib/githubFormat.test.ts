import { describe, expect, it } from "vitest";
import type { GithubPr, GithubPrsData } from "./githubTypes";
import {
  ciLabel,
  ciTone,
  ciToneColor,
  filterPrs,
  formatUpdatedAgo,
  listCountLabel,
  mergeBadge,
  orderPrs,
  prsSummary,
  reviewDecisionLabel,
  reviewDecisionTone,
  statusHeadline,
} from "./githubFormat";

const NOW = Date.parse("2026-08-24T20:00:00Z");

function pr(overrides: Partial<GithubPr>): GithubPr {
  return {
    id: "PR_1",
    number: 1,
    title: "t",
    url: "https://github.com/acme/app/pull/1",
    repo: "acme/app",
    author: "octocat",
    isDraft: false,
    ci: "none",
    headRef: "h",
    baseRef: "main",
    updatedAt: "2026-08-24T19:00:00Z",
    mergeable: "mergeable",
    mergeState: "blocked",
    autoMerge: false,
    ...overrides,
  };
}

describe("githubFormat", () => {
  it("maps CI states to the app's tones and labels", () => {
    expect(ciTone("success")).toBe("live");
    expect(ciTone("failure")).toBe("trip");
    expect(ciTone("error")).toBe("trip");
    expect(ciTone("pending")).toBe("warn");
    expect(ciTone("none")).toBe("mute");
    expect(ciToneColor("live")).toBe("var(--live)");
    expect(ciToneColor("trip")).toBe("var(--trip)");
    expect(ciToneColor("warn")).toBe("var(--warn)");
    expect(ciToneColor("mute")).toBe("var(--mute)");
    expect(ciLabel("success")).toBe("CI passing");
    expect(ciLabel("failure")).toBe("CI failing");
    expect(ciLabel("error")).toBe("CI errored");
    expect(ciLabel("pending")).toBe("CI pending");
    expect(ciLabel("none")).toBe("No checks");
  });

  it("puts failing CI first, then pending, then the most recently updated", () => {
    const ordered = orderPrs([
      pr({ id: "ok-old", ci: "success", updatedAt: "2026-08-24T10:00:00Z" }),
      pr({ id: "pending", ci: "pending", updatedAt: "2026-08-24T12:00:00Z" }),
      pr({ id: "ok-new", ci: "success", updatedAt: "2026-08-24T19:00:00Z" }),
      pr({ id: "failing", ci: "failure", updatedAt: "2026-08-24T08:00:00Z" }),
      pr({ id: "errored", ci: "error", updatedAt: "2026-08-24T09:00:00Z" }),
      pr({ id: "none", ci: "none", updatedAt: "2026-08-24T18:00:00Z" }),
    ]).map((item) => item.id);
    expect(ordered).toEqual(["errored", "failing", "pending", "ok-new", "none", "ok-old"]);
  });

  it("describes how long ago a pull request moved", () => {
    expect(formatUpdatedAgo("2026-08-24T19:59:30Z", NOW)).toBe("just now");
    expect(formatUpdatedAgo("2026-08-24T19:55:00Z", NOW)).toBe("5m ago");
    expect(formatUpdatedAgo("2026-08-24T17:10:00Z", NOW)).toBe("2h ago");
    expect(formatUpdatedAgo("2026-08-21T20:00:00Z", NOW)).toBe("3d ago");
    expect(formatUpdatedAgo("not a date", NOW)).toBe("");
  });

  it("matches a search against number, title, repository, author and branches", () => {
    const items = [
      pr({ id: "a", number: 412, title: "Add retry to the sync worker", repo: "acme/web", author: "you", headRef: "feat/retry" }),
      pr({ id: "b", number: 91, title: "api: reject unsigned webhooks", repo: "acme/api", author: "lin", headRef: "fix/webhooks", baseRef: "release" }),
    ];
    const ids = (query: string) => filterPrs(items, query).map((item) => item.id);
    expect(ids("")).toEqual(["a", "b"]);
    expect(ids("   ")).toEqual(["a", "b"]);
    expect(ids("#412")).toEqual(["a"]);
    expect(ids("91")).toEqual(["b"]);
    expect(ids("SYNC")).toEqual(["a"]);
    expect(ids("acme/api")).toEqual(["b"]);
    expect(ids("lin")).toEqual(["b"]);
    expect(ids("fix/web")).toEqual(["b"]);
    expect(ids("release")).toEqual(["b"]);
    expect(ids("nothing here")).toEqual([]);
  });

  it("matches the words a row shows as badges and CI state", () => {
    const items = [
      pr({ id: "draft", isDraft: true, ci: "pending" }),
      pr({ id: "team", reviewRequest: "team", ci: "failure" }),
      pr({ id: "approved", reviewDecision: "APPROVED", ci: "success", mergeState: "clean" }),
      pr({ id: "changes", reviewDecision: "CHANGES_REQUESTED", ci: "error" }),
      pr({ id: "conflicts", mergeable: "conflicting" }),
      pr({ id: "queued", mergeQueue: { position: 2 } }),
    ];
    const ids = (query: string) => filterPrs(items, query).map((item) => item.id);
    expect(ids("draft")).toEqual(["draft"]);
    expect(ids("team")).toEqual(["team"]);
    expect(ids("approved")).toEqual(["approved"]);
    expect(ids("changes requested")).toEqual(["changes"]);
    expect(ids("failing")).toEqual(["team"]);
    expect(ids("pending")).toEqual(["draft"]);
    expect(ids("errored")).toEqual(["changes"]);
    expect(ids("conflicts")).toEqual(["conflicts"]);
    expect(ids("queued #2")).toEqual(["queued"]);
    expect(ids("ready to merge")).toEqual(["approved"]);
  });

  it("gives each pull request one merge badge, the most pressing state first", () => {
    const badge = (overrides: Partial<GithubPr>) => mergeBadge(pr(overrides));
    // Conflicts beat everything, whichever field reports them.
    expect(badge({ mergeable: "conflicting", mergeQueue: { position: 1 }, mergeState: "clean" })).toEqual({ label: "Conflicts", tone: "trip" });
    expect(badge({ mergeState: "dirty" })).toEqual({ label: "Conflicts", tone: "trip" });
    // The merge queue, with its position when GitHub reports one.
    expect(badge({ mergeQueue: { position: 3 }, autoMerge: true, mergeState: "clean" })).toEqual({ label: "Queued #3", tone: "live" });
    expect(badge({ mergeQueue: { position: null } })).toEqual({ label: "Queued", tone: "live" });
    expect(badge({ mergeQueue: {} })).toEqual({ label: "Queued", tone: "live" });
    // Auto-merge will merge once the requirements are met.
    expect(badge({ autoMerge: true, mergeState: "clean" })).toEqual({ label: "Auto-merge", tone: "mute" });
    expect(badge({ autoMerge: true, mergeState: "blocked", reviewDecision: "REVIEW_REQUIRED" })).toEqual({ label: "Auto-merge", tone: "mute" });
    // Every requirement met; a draft is never "ready".
    expect(badge({ mergeState: "clean" })).toEqual({ label: "Ready to merge", tone: "live" });
    expect(badge({ mergeState: "clean", isDraft: true })).toBeNull();
    expect(badge({ mergeState: "behind" })).toEqual({ label: "Behind base", tone: "warn" });
    // "Blocked" only when the review and CI badges do not already say why.
    expect(badge({ mergeState: "blocked", reviewDecision: "APPROVED", ci: "success" })).toEqual({ label: "Blocked", tone: "warn" });
    expect(badge({ mergeState: "blocked", reviewDecision: null, ci: "none" })).toEqual({ label: "Blocked", tone: "warn" });
    expect(badge({ mergeState: "blocked", reviewDecision: "REVIEW_REQUIRED", ci: "success" })).toBeNull();
    expect(badge({ mergeState: "blocked", reviewDecision: "CHANGES_REQUESTED", ci: "success" })).toBeNull();
    expect(badge({ mergeState: "blocked", reviewDecision: "APPROVED", ci: "failure" })).toBeNull();
    expect(badge({ mergeState: "blocked", reviewDecision: "APPROVED", ci: "pending" })).toBeNull();
    // Nothing to say yet.
    expect(badge({ mergeState: "unknown", mergeable: "unknown" })).toBeNull();
    expect(badge({ mergeState: "unstable" })).toBeNull();
    expect(badge({ mergeState: "draft", isDraft: true })).toBeNull();
  });

  it("says how much of a long list is loaded, and how much of it matches a search", () => {
    expect(listCountLabel({ total: 3, items: [pr({}), pr({}), pr({})] })).toBe("3");
    expect(listCountLabel({ total: 137, items: [pr({})] })).toBe("1 of 137");
    expect(listCountLabel({ total: 0, items: [] })).toBe("0");
    expect(listCountLabel({ total: 3, items: [pr({}), pr({}), pr({})] }, 1)).toBe("1 of 3");
    // Only the loaded page was searched, so the denominator is the page, and says so.
    expect(listCountLabel({ total: 137, items: [pr({})] }, 0)).toBe("0 of 1 loaded");
  });

  it("gives every problem a headline", () => {
    expect(statusHeadline("ok")).toBe("");
    expect(statusHeadline("ghMissing")).toBe("GitHub CLI not found");
    expect(statusHeadline("ghNotLoggedIn")).toBe("Not signed in to GitHub");
    expect(statusHeadline("tokenRejected")).toBe("GitHub sign-in rejected");
    expect(statusHeadline("rateLimited")).toBe("GitHub rate limit reached");
    expect(statusHeadline("network")).toBe("GitHub unreachable");
  });

  it("colours review decisions like CI states", () => {
    expect(reviewDecisionTone("APPROVED")).toBe("live");
    expect(reviewDecisionTone("CHANGES_REQUESTED")).toBe("trip");
    expect(reviewDecisionTone("REVIEW_REQUIRED")).toBe("mute");
    expect(reviewDecisionTone(null)).toBe("mute");
  });

  it("labels only the review decisions that carry information", () => {
    expect(reviewDecisionLabel("APPROVED")).toBe("Approved");
    expect(reviewDecisionLabel("CHANGES_REQUESTED")).toBe("Changes requested");
    expect(reviewDecisionLabel("REVIEW_REQUIRED")).toBe("");
    expect(reviewDecisionLabel(null)).toBe("");
    expect(reviewDecisionLabel(undefined)).toBe("");
  });

  it("summarises the three lists in one line, naming failing CI on own pull requests", () => {
    const data: GithubPrsData = {
      scope: [],
      mine: { total: 3, items: [pr({ ci: "failure" }), pr({ ci: "error" }), pr({ ci: "success" })] },
      reviewRequested: { total: 15, items: [pr({ ci: "failure" })] },
      assigned: { total: 0, items: [] },
    };
    expect(prsSummary(data)).toEqual({ mine: 3, failing: 2, conflicts: 0, ready: 0, failingIsPartial: false, review: 15, assigned: 0 });
    expect(prsSummary({ ...data, mine: { total: 0, items: [] } })).toEqual({
      mine: 0,
      failing: 0,
      conflicts: 0,
      ready: 0,
      failingIsPartial: false,
      review: 15,
      assigned: 0,
    });
    // With more on GitHub than was loaded, the failing count covers only the loaded page.
    expect(prsSummary({ ...data, mine: { total: 137, items: data.mine.items } })).toEqual({
      mine: 137,
      failing: 2,
      conflicts: 0,
      ready: 0,
      failingIsPartial: true,
      review: 15,
      assigned: 0,
    });
  });

  it("counts own pull requests with conflicts and ready to merge", () => {
    const data: GithubPrsData = {
      scope: [],
      mine: {
        total: 4,
        items: [
          pr({ mergeable: "conflicting" }),
          pr({ mergeState: "dirty" }),
          pr({ mergeState: "clean", ci: "success" }),
          pr({ mergeState: "clean", isDraft: true }),
        ],
      },
      // Conflicts on the review list are the author's problem, not the reviewer's.
      reviewRequested: { total: 1, items: [pr({ mergeable: "conflicting" })] },
      assigned: { total: 0, items: [] },
    };
    expect(prsSummary(data)).toMatchObject({ conflicts: 2, ready: 1 });
  });
});
