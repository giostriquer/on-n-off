import { describe, expect, it } from "vitest";
import { githubQueryOptions } from "./useGithubPrs";

describe("githubQueryOptions", () => {
  it("polls on the configured interval only while the window is visible", () => {
    const options = githubQueryOptions(30, () => Promise.reject(new Error("unused")));
    expect(options.queryKey).toEqual(["github", "prs"]);
    expect(options.refetchInterval).toBe(30_000);
    expect(options.refetchIntervalInBackground).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(true);
  });
});
