import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GithubPr, GithubPrs, GithubStatus } from "$lib/githubTypes";
import { Github } from "./Github";

const readGithubPrs = vi.hoisted(() => vi.fn());
const openUrl = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({ readGithubPrs, openUrl }));

const NOW = "2026-08-24T20:00:00Z";

function pr(overrides: Partial<GithubPr> = {}): GithubPr {
  return {
    id: "PR_41",
    number: 41,
    title: "Add the thing",
    url: "https://github.com/acme/app/pull/41",
    repo: "acme/app",
    author: "octocat",
    isDraft: false,
    reviewDecision: null,
    ci: "failure",
    headRef: "feat/thing",
    baseRef: "main",
    updatedAt: "2026-08-24T19:55:00Z",
    mergeable: "mergeable",
    mergeState: "blocked",
    autoMerge: false,
    ...overrides,
  };
}

function okPrs(overrides: Partial<GithubPrs> = {}): GithubPrs {
  return {
    status: "ok",
    stale: false,
    viewer: "octocat",
    fetchedAt: NOW,
    scope: ["org:acme"],
    mine: { total: 1, items: [pr()] },
    reviewRequested: {
      total: 2,
      items: [
        pr({
          id: "PR_7",
          number: 7,
          title: "Direct ask",
          url: "https://github.com/acme/lib/pull/7",
          repo: "acme/lib",
          author: "alice",
          isDraft: true,
          ci: "pending",
          reviewRequest: "direct",
        }),
        pr({
          id: "PR_8",
          number: 8,
          title: "Team ask",
          url: "https://github.com/acme/lib/pull/8",
          repo: "acme/lib",
          author: "bob",
          reviewDecision: "APPROVED",
          ci: "none",
          reviewRequest: "team",
          mergeable: "conflicting",
          mergeState: "dirty",
        }),
      ],
    },
    assigned: { total: 0, items: [] },
    rateLimit: { remaining: 4998, resetAt: "2026-08-24T23:00:00Z" },
    ...overrides,
  };
}

function problem(status: GithubStatus, hint: string, overrides: Partial<GithubPrs> = {}): GithubPrs {
  return {
    status,
    hint,
    stale: false,
    scope: [],
    mine: { total: 0, items: [] },
    reviewRequested: { total: 0, items: [] },
    assigned: { total: 0, items: [] },
    ...overrides,
  };
}

function renderGithub(pollSeconds: 30 | 60 | 120 | 300 = 60) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <Github pollSeconds={pollSeconds} />
    </QueryClientProvider>,
  );
  return { ...view, client };
}

function section(name: string) {
  return screen.getByRole("region", { name });
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  localStorage.clear();
  readGithubPrs.mockReset();
  openUrl.mockReset();
  openUrl.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Github", () => {
  it("lists authored, review-requested and assigned pull requests with their CI state", async () => {
    readGithubPrs.mockResolvedValue(okPrs());
    renderGithub();

    await screen.findByText("Add the thing");
    expect(screen.getByRole("heading", { level: 2, name: "Pull requests" })).toBeTruthy();
    const meta = screen.getByTestId("github-caption").textContent ?? "";
    expect(meta).toContain("octocat");
    expect(meta).toContain("updated just now");
    expect(meta).toContain("every 60 s");
    expect(screen.getByTestId("github-summary").textContent).toBe("1 mine · 1 failing · 2 to review · 0 assigned");
    expect(screen.getByText("org:acme")).toBeTruthy();

    expect(screen.getAllByRole("region").map((region) => region.getAttribute("aria-label"))).toEqual([
      "Mine",
      "Review requested",
      "Assigned",
    ]);

    const mine = section("Mine");
    // Every row of the list lives in one repository, so it describes the title row and the rows
    // do not repeat it.
    expect(within(mine).getByRole("heading", { level: 3 }).textContent).toBe("Mine1· acme/app");
    expect(within(mine).queryByRole("group")).toBeNull();
    expect(within(mine).getByText("#41")).toBeTruthy();
    expect(within(mine).getByText("Add the thing")).toBeTruthy();
    expect(within(mine).queryByText("acme/app")).toBeNull();
    expect(within(mine).queryByText("app")).toBeNull();
    expect(within(mine).getByText("octocat")).toBeTruthy();
    expect(within(mine).getByText("feat/thing → main")).toBeTruthy();
    const age = within(mine).getByText("5m ago");
    expect(age.tagName).toBe("TIME");
    expect(age.getAttribute("datetime")).toBe("2026-08-24T19:55:00Z");
    expect(age.getAttribute("title")).toMatch(/2026/);
    expect(within(mine).getByRole("button", { name: /CI failing/ })).toBeTruthy();
    expect(within(mine).queryByText("Review required")).toBeNull();

    const review = section("Review requested");
    expect(within(review).getByRole("heading", { level: 3 }).textContent).toBe("Review requested2· acme/lib");
    expect(within(review).queryByText("acme/lib")).toBeNull();
    expect(within(review).getByText("Draft")).toBeTruthy();
    expect(within(review).getByText("team")).toBeTruthy();
    expect(within(review).getByText("Approved")).toBeTruthy();
    expect(within(review).getByText("Conflicts")).toBeTruthy();
    expect(within(mine).queryByText("Blocked")).toBeNull();
    expect(within(review).getByRole("button", { name: /CI pending/ })).toBeTruthy();
    expect(within(review).getByRole("button", { name: /No checks/ })).toBeTruthy();

    expect(within(section("Assigned")).getByText("Nothing assigned to you.")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("opens the pull request and its checks in the browser", async () => {
    const user = userEvent.setup({ advanceTimers: () => undefined });
    readGithubPrs.mockResolvedValue(okPrs());
    renderGithub();
    await screen.findByText("Add the thing");

    await user.click(within(section("Mine")).getByRole("button", { name: /#41.*Add the thing.*octocat/ }));
    expect(openUrl).toHaveBeenCalledWith("https://github.com/acme/app/pull/41");

    await user.click(within(section("Mine")).getByRole("button", { name: /CI failing/ }));
    expect(openUrl).toHaveBeenLastCalledWith("https://github.com/acme/app/pull/41/checks");
  });

  it("puts failing CI first and says how much of a long list is loaded", async () => {
    readGithubPrs.mockResolvedValue(
      okPrs({
        mine: {
          total: 137,
          items: [
            pr({ id: "a", number: 1, title: "Green", ci: "success", updatedAt: "2026-08-24T19:59:00Z" }),
            pr({ id: "b", number: 2, title: "Red", ci: "failure", updatedAt: "2026-08-24T19:00:00Z" }),
            pr({ id: "c", number: 3, title: "Amber", ci: "pending", updatedAt: "2026-08-24T19:30:00Z" }),
          ],
        },
      }),
    );
    renderGithub();
    await screen.findByText("Red");

    const mine = section("Mine");
    expect(within(mine).getByRole("heading", { level: 3 }).textContent).toContain("3 of 137");
    const titles = within(mine)
      .getAllByRole("listitem")
      .map((item) => item.textContent ?? "");
    expect(titles[0]).toContain("Red");
    expect(titles[1]).toContain("Amber");
    expect(titles[2]).toContain("Green");
  });

  it.each<[GithubStatus, string, string]>([
    ["ghMissing", "GitHub CLI not found", "Install the GitHub CLI (`gh`), sign in with `gh auth login`, then refresh."],
    ["ghNotLoggedIn", "Not signed in to GitHub", "Sign in with `gh auth login` in a terminal, then refresh."],
    ["tokenRejected", "GitHub sign-in rejected", "GitHub rejected the `gh` login — run `gh auth login` again, then refresh."],
    ["rateLimited", "GitHub rate limit reached", "GitHub rate limit reached — polling resumes in 5 min."],
    ["network", "GitHub unreachable", "Could not reach GitHub (network error: Dns)."],
  ])("explains a %s problem with the backend's hint", async (status, headline, hint) => {
    readGithubPrs.mockResolvedValue(problem(status, hint));
    renderGithub();

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain(headline);
    expect(banner.textContent).toContain(hint);
    expect(within(section("Mine")).getByText("No open pull requests of yours.")).toBeTruthy();
  });

  it("keeps showing the last read when a refresh fails", async () => {
    readGithubPrs.mockResolvedValue(
      okPrs({
        status: "network",
        hint: "Could not reach GitHub (network error: Dns).",
        stale: true,
        fetchedAt: "2026-08-24T18:00:00Z",
      }),
    );
    renderGithub();

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("GitHub unreachable");
    expect(screen.getByText(/Showing the last read from 2h ago/)).toBeTruthy();
    expect(within(section("Mine")).getByText("Add the thing")).toBeTruthy();
  });

  it("reports a backend failure without hiding the screen", async () => {
    readGithubPrs.mockRejectedValue({ kind: "message", message: "github read worker failed", path: null });
    renderGithub();

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("github read worker failed");
    expect(section("Mine")).toBeTruthy();
  });

  it("forces one backend read when the refresh button is pressed", async () => {
    const user = userEvent.setup({ advanceTimers: () => undefined });
    readGithubPrs.mockResolvedValue(okPrs());
    const { client } = renderGithub();
    await screen.findByText("Add the thing");
    expect(readGithubPrs).toHaveBeenLastCalledWith(false);

    await user.click(screen.getByRole("button", { name: "Refresh pull requests" }));
    await waitFor(() => expect(readGithubPrs).toHaveBeenLastCalledWith(true));
    expect(readGithubPrs).toHaveBeenCalledTimes(2);

    await act(async () => {
      await client.refetchQueries({ queryKey: ["github", "prs"] });
    });
    expect(readGithubPrs).toHaveBeenCalledTimes(3);
    expect(readGithubPrs).toHaveBeenLastCalledWith(false);
  });

  it("keeps the previous lists when a later read fails outright", async () => {
    readGithubPrs.mockResolvedValue(okPrs());
    const { client } = renderGithub();
    await screen.findByText("Add the thing");

    readGithubPrs.mockRejectedValue({ kind: "message", message: "github read worker failed", path: null });
    await act(async () => {
      await client.refetchQueries({ queryKey: ["github", "prs"] });
    });

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("GitHub unavailable");
    expect(banner.textContent).toContain("github read worker failed");
    expect(within(section("Mine")).getByText("Add the thing")).toBeTruthy();
  });

  it("filters every list from one search field and says how many matched", async () => {
    const user = userEvent.setup({ advanceTimers: () => undefined });
    readGithubPrs.mockResolvedValue(okPrs());
    renderGithub();
    await screen.findByText("Add the thing");

    const search = screen.getByRole("searchbox", { name: "Search pull requests" });
    await user.type(search, "acme/lib");

    expect(within(section("Mine")).getByRole("heading", { level: 3 }).textContent).toContain("0 of 1");
    expect(within(section("Mine")).getByText("No matches.")).toBeTruthy();
    expect(within(section("Review requested")).getByRole("heading", { level: 3 }).textContent).toContain("2 of 2");
    expect(within(section("Mine")).queryByText("Add the thing")).toBeNull();

    await user.clear(search);
    await user.type(search, "team ask");
    expect(within(section("Review requested")).getAllByRole("listitem")).toHaveLength(1);
    expect(within(section("Review requested")).getByText("Team ask")).toBeTruthy();

    await user.keyboard("{Escape}");
    expect(search).toHaveValue("");
    expect(within(section("Mine")).getByText("Add the thing")).toBeTruthy();
  });

  it("collapses a section from its header and remembers it", async () => {
    const user = userEvent.setup({ advanceTimers: () => undefined });
    readGithubPrs.mockResolvedValue(okPrs());
    const first = renderGithub();
    await screen.findByText("Add the thing");

    const toggle = within(section("Review requested")).getByRole("button", { name: /Review requested/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    await user.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(within(section("Review requested")).queryByText("Direct ask")).toBeNull();
    // Folded, the title row still says how many and from which repository.
    expect(within(section("Review requested")).getByRole("heading", { level: 3 }).textContent).toBe("Review requested2· acme/lib");
    expect(within(section("Mine")).getByText("Add the thing")).toBeTruthy();

    first.unmount();
    renderGithub();
    await screen.findByText("Add the thing");
    expect(
      within(section("Review requested")).getByRole("button", { name: /Review requested/ }).getAttribute("aria-expanded"),
    ).toBe("false");
    expect(within(section("Review requested")).queryByText("Direct ask")).toBeNull();
    await user.click(within(section("Review requested")).getByRole("button", { name: /Review requested/ }));
    expect(within(section("Review requested")).getByText("Direct ask")).toBeTruthy();
  });

  it("opens a folded section while a search matches inside it", async () => {
    const user = userEvent.setup({ advanceTimers: () => undefined });
    readGithubPrs.mockResolvedValue(okPrs());
    renderGithub();
    await screen.findByText("Add the thing");

    const toggle = () => within(section("Review requested")).getByRole("button", { name: /Review requested/ });
    await user.click(toggle());
    expect(within(section("Review requested")).queryByText("Team ask")).toBeNull();

    const search = screen.getByRole("searchbox", { name: "Search pull requests" });
    await user.type(search, "team ask");
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    expect(within(section("Review requested")).getByText("Team ask")).toBeTruthy();

    await user.clear(search);
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    expect(within(section("Review requested")).queryByText("Team ask")).toBeNull();
  });

  it("names own pull requests with conflicts or ready to merge in the summary, with their badges", async () => {
    readGithubPrs.mockResolvedValue(
      okPrs({
        mine: {
          total: 4,
          items: [
            pr(),
            pr({ id: "PR_42", number: 42, title: "Conflicted", mergeable: "conflicting", ci: "success" }),
            pr({ id: "PR_43", number: 43, title: "Green and approved", reviewDecision: "APPROVED", mergeState: "clean", ci: "success" }),
            pr({ id: "PR_44", number: 44, title: "In the queue", reviewDecision: "APPROVED", mergeQueue: { position: 2 }, ci: "success" }),
          ],
        },
      }),
    );
    renderGithub();

    await screen.findByText("Conflicted");
    expect(screen.getByTestId("github-summary").textContent).toBe(
      "4 mine · 1 failing · 1 with conflicts · 1 ready · 2 to review · 0 assigned",
    );
    const mine = section("Mine");
    expect(within(mine).getByText("Conflicts").style.color).toBe("var(--trip)");
    expect(within(mine).getByText("Ready to merge").style.color).toBe("var(--live)");
    expect(within(mine).getByText("Queued #2").style.color).toBe("var(--live)");
  });

  it("groups a list that spans several repositories under one sub-heading per repository", async () => {
    readGithubPrs.mockResolvedValue(
      okPrs({
        reviewRequested: {
          total: 3,
          items: [
            pr({ id: "PR_t1", number: 305, title: "tools: bump lockfile", url: "https://github.com/octo/tools/pull/305", repo: "octo/tools", author: "sam", ci: "none" }),
            pr({ id: "PR_7", number: 7, title: "Direct ask", url: "https://github.com/acme/lib/pull/7", repo: "acme/lib", author: "alice", ci: "pending" }),
            pr({ id: "PR_91", number: 91, title: "api: paginate", url: "https://github.com/acme/api/pull/91", repo: "acme/api", author: "lin", ci: "success" }),
          ],
        },
      }),
    );
    const user = userEvent.setup({ advanceTimers: () => undefined });
    renderGithub();

    await screen.findByText("Direct ask");
    const review = section("Review requested");
    expect(within(review).getByRole("heading", { level: 3 }).textContent).toBe("Review requested3");
    const groups = within(review).getAllByRole("group");
    expect(groups.map((group) => group.getAttribute("aria-label"))).toEqual(["acme/api", "acme/lib", "octo/tools"]);
    const controlled = within(review).getByRole("button", { name: /Review requested/ }).getAttribute("aria-controls");
    expect(document.getElementById(controlled ?? "")?.contains(groups[0])).toBe(true);
    expect(within(groups[0]).getByRole("heading", { level: 4 }).textContent).toBe("acme/api1");
    expect(within(groups[0]).getByRole("listitem").textContent).toContain("api: paginate");
    expect(within(groups[1]).getByRole("heading", { level: 4 }).textContent).toBe("acme/lib1");
    expect(within(groups[1]).getByRole("listitem").textContent).toContain("Direct ask");
    expect(within(groups[2]).getByRole("heading", { level: 4 }).textContent).toBe("octo/tools1");
    // The repository is said once, by the band, never again on the row.
    const row = within(groups[2]).getByRole("listitem");
    expect(within(row).getByText("sam")).toBeTruthy();
    expect(within(row).queryByText("octo/tools")).toBeNull();
    expect(within(row).queryByText("tools")).toBeNull();
    // A search still matches the repository and narrows the groups to the ones with hits.
    await user.type(screen.getByRole("searchbox"), "octo/");
    expect(within(review).getByRole("heading", { level: 3 }).textContent).toBe("Review requested1 of 3· octo/tools");
    expect(within(review).queryByRole("group")).toBeNull();
    expect(within(review).getByText("tools: bump lockfile")).toBeTruthy();
  });

  it("marks every own-list count as partial when GitHub holds more than was loaded", async () => {
    readGithubPrs.mockResolvedValue(
      okPrs({
        mine: {
          total: 137,
          items: [
            pr({ id: "a", ci: "failure" }),
            pr({ id: "b", number: 2, title: "Second thing", ci: "success" }),
            pr({ id: "c", number: 3, title: "Conflicted", mergeable: "conflicting", ci: "success" }),
            pr({ id: "d", number: 4, title: "Ready", mergeState: "clean", reviewDecision: "APPROVED", ci: "success" }),
          ],
        },
      }),
    );
    renderGithub();
    await screen.findByText("Second thing");
    expect(screen.getByTestId("github-summary").textContent).toBe(
      "137 mine · 1+ failing · 1+ with conflicts · 1+ ready · 2 to review · 0 assigned",
    );
    expect(within(section("Mine")).getByRole("heading", { level: 3 }).textContent).toContain("4 of 137");
  });

  it("says it is checking until the first read answers", () => {
    readGithubPrs.mockReturnValue(new Promise(() => undefined));
    renderGithub();

    expect(screen.getByRole("status").textContent).toContain("Checking…");
    expect(within(section("Mine")).getByText("Checking…")).toBeTruthy();
    expect(within(section("Review requested")).getByText("Checking…")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByTestId("github-summary")).toBeNull();
  });
});
