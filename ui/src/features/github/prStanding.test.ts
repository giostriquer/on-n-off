import { expect, it } from "vitest";
import { GITHUB_STATUSES, type GithubPrs } from "$lib/githubTypes";
import { prBanner, showsLastRead } from "./prStanding";
import { PR_STANDING_CASES, type PrStandingCase } from "./prStandingCases";

function sent({ status, standing }: PrStandingCase): GithubPrs {
  return {
    status,
    scope: [],
    mine: { total: 0, items: [] },
    reviewRequested: { total: 0, items: [] },
    assigned: { total: 0, items: [] },
    merged: { total: 0, items: [] },
    ...(standing.kind === "live" ? {} : { standing }),
  };
}

it.each(PR_STANDING_CASES)("shows $name as its read standing says", (row) => {
  const prs = sent(row);
  expect({ banner: prBanner(prs), lastRead: showsLastRead(prs) }).toEqual(row.screen);
});

it("names every status the read sends, and only those", () => {
  expect(new Set(PR_STANDING_CASES.map(({ status }) => status))).toEqual(new Set(GITHUB_STATUSES));
});

it("reads a reply without a standing as live, whatever its status says", () => {
  const failed = PR_STANDING_CASES.find(({ standing }) => standing.kind === "lastObserved");
  if (!failed) throw new Error("a failed read over the last one among the cases");
  const prs = { ...sent(failed), standing: undefined };
  expect({ banner: prBanner(prs), lastRead: showsLastRead(prs) }).toEqual({ banner: null, lastRead: false });
});
