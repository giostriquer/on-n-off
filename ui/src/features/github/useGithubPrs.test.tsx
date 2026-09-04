import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SharedReadChanged } from "$lib/types";
import { githubQueryOptions, useGithubPrs } from "./useGithubPrs";

const calls = vi.hoisted(() => ({
  readGithubPrs: vi.fn(),
  listeners: new Set<(change: SharedReadChanged) => void>(),
}));
vi.mock("$lib/api", () => ({
  readGithubPrs: calls.readGithubPrs,
  onSharedReadChanged: async (listener: (change: SharedReadChanged) => void) => {
    calls.listeners.add(listener);
    return () => calls.listeners.delete(listener);
  },
}));

describe("githubQueryOptions", () => {
  it("polls on the configured interval only while the window is visible", () => {
    const options = githubQueryOptions(30, () => Promise.reject(new Error("unused")));
    expect(options.queryKey).toEqual(["github", "prs"]);
    expect(options.refetchInterval).toBe(30_000);
    expect(options.refetchIntervalInBackground).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(true);
  });

  it("picks up a read another surface already made, unforced, without waiting out the interval", async () => {
    calls.listeners.clear();
    calls.readGithubPrs.mockReset().mockResolvedValue({ status: "ok" });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    renderHook(() => useGithubPrs(60), { wrapper });
    await waitFor(() => expect(calls.readGithubPrs).toHaveBeenCalledTimes(1));

    for (const listener of calls.listeners) listener({ source: "github:prs" });

    await waitFor(() => expect(calls.readGithubPrs).toHaveBeenCalledTimes(2));
    expect(calls.readGithubPrs).toHaveBeenLastCalledWith(false);
  });
});
