import { describe, expect, it } from "vitest";
import type { GithubPr, GithubPrsData } from "./githubTypes";
import {
  ciLabel,
  ciTone,
  ciToneColor,
  filterPrs,
  listCountLabel,
  orderPrs,
  prsSummary,
  reviewDecisionLabel,
  reviewDecisionTone,
  statusHeadline,
} from "./githubFormat";

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
      pr({ id: "approved", reviewDecision: "APPROVED", ci: "success" }),
      pr({ id: "changes", reviewDecision: "CHANGES_REQUESTED", ci: "error" }),
    ];
    const ids = (query: string) => filterPrs(items, query).map((item) => item.id);
    expect(ids("draft")).toEqual(["draft"]);
    expect(ids("team")).toEqual(["team"]);
    expect(ids("approved")).toEqual(["approved"]);
    expect(ids("changes requested")).toEqual(["changes"]);
    expect(ids("failing")).toEqual(["team"]);
    expect(ids("pending")).toEqual(["draft"]);
    expect(ids("errored")).toEqual(["changes"]);
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
    expect(prsSummary(data)).toEqual({ mine: 3, failing: 2, failingIsPartial: false, review: 15, assigned: 0 });
    expect(prsSummary({ ...data, mine: { total: 0, items: [] } })).toEqual({
      mine: 0,
      failing: 0,
      failingIsPartial: false,
      review: 15,
      assigned: 0,
    });
    // With more on GitHub than was loaded, the failing count covers only the loaded page.
    expect(prsSummary({ ...data, mine: { total: 137, items: data.mine.items } })).toEqual({
      mine: 137,
      failing: 2,
      failingIsPartial: true,
      review: 15,
      assigned: 0,
    });
  });
});
