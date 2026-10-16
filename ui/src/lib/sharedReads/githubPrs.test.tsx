import type { ReactNode } from "react";
import { focusManager, QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SharedReadChanged } from "$lib/types";
import { SharedReads } from "./announcements";
import { githubPrsQuery } from "./githubPrs";

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

function windowWith(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}><SharedReads />{children}</QueryClientProvider>
  );
}

describe("the pull requests read", () => {
  it("polls on the configured interval only while the window is visible, and reads again on focus", async () => {
    vi.useFakeTimers();
    calls.readGithubPrs.mockReset().mockResolvedValue({ status: "ok" });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const hook = renderHook(() => useQuery(githubPrsQuery(30)), { wrapper: windowWith(client) });
    try {
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(calls.readGithubPrs.mock.calls).toEqual([[false]]);
      await act(() => vi.advanceTimersByTimeAsync(29_000));
      expect(calls.readGithubPrs).toHaveBeenCalledTimes(1);
      await act(() => vi.advanceTimersByTimeAsync(1_000));
      expect(calls.readGithubPrs.mock.calls).toEqual([[false], [false]]);

      act(() => focusManager.setFocused(false));
      await act(() => vi.advanceTimersByTimeAsync(60_000));
      expect(calls.readGithubPrs).toHaveBeenCalledTimes(2);
      act(() => focusManager.setFocused(true));
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(calls.readGithubPrs.mock.calls).toEqual([[false], [false], [false]]);
    } finally {
      focusManager.setFocused(undefined);
      hook.unmount();
      client.clear();
      vi.useRealTimers();
    }
  });

});
