import { describe, expect, it } from "vitest";
import type { GithubPr, GithubPrsData } from "./githubTypes";
import {
  ciLabel,
  ciTone,
  ciToneColor,
  formatUpdatedAgo,
  listCountLabel,
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

  it("says how much of a long list is loaded", () => {
    expect(listCountLabel({ total: 3, items: [pr({}), pr({}), pr({})] })).toBe("3");
    expect(listCountLabel({ total: 137, items: [pr({})] })).toBe("1 of 137");
    expect(listCountLabel({ total: 0, items: [] })).toBe("0");
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
    expect(prsSummary(data)).toEqual({ mine: 3, failing: 2, review: 15, assigned: 0 });
    expect(prsSummary({ ...data, mine: { total: 0, items: [] } })).toEqual({
      mine: 0,
      failing: 0,
      review: 15,
      assigned: 0,
    });
  });
});
