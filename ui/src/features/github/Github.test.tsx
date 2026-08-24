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
    expect(screen.getByTestId("github-caption").textContent).toContain("octocat");
    expect(screen.getByTestId("github-caption").textContent).toContain("updated just now");
    expect(screen.getByText("org:acme")).toBeTruthy();

    const mine = section("Mine");
    expect(within(mine).getByRole("heading").textContent).toContain("1");
    expect(within(mine).getByText("acme/app#41")).toBeTruthy();
    expect(within(mine).getByText("Add the thing")).toBeTruthy();
    expect(within(mine).getByText("feat/thing → main")).toBeTruthy();
    expect(within(mine).getByText("5m ago")).toBeTruthy();
    expect(within(mine).getByRole("button", { name: /CI failing/ })).toBeTruthy();

    const review = section("Review requested");
    expect(within(review).getByRole("heading").textContent).toContain("2");
    expect(within(review).getByText("Draft")).toBeTruthy();
    expect(within(review).getByText("team")).toBeTruthy();
    expect(within(review).getByText("Approved")).toBeTruthy();
    expect(within(review).getByRole("button", { name: /CI pending/ })).toBeTruthy();
    expect(within(review).getByRole("button", { name: /No checks/ })).toBeTruthy();

    expect(within(section("Assigned")).getByText("Nothing assigned to you.")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("opens the pull request and its checks in the browser", async () => {
    const user = userEvent.setup({ advanceTimers: () => undefined });
    readGithubPrs.mockResolvedValue(okPrs());
    renderGithub();
    await screen.findByText("Add the thing");

    await user.click(within(section("Mine")).getByRole("button", { name: /acme\/app#41.*Add the thing/ }));
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
    expect(within(mine).getByRole("heading").textContent).toContain("3 of 137");
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

  it("says it is checking until the first read answers", () => {
    readGithubPrs.mockReturnValue(new Promise(() => undefined));
    renderGithub();

    expect(screen.getByRole("status").textContent).toContain("Checking…");
    expect(within(section("Mine")).getByText("Checking…")).toBeTruthy();
    expect(within(section("Review requested")).getByText("Checking…")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
