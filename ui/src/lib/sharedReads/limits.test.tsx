import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId, SharedReadChanged } from "$lib/types";
import { SharedReads } from "./announcements";
import { LIMITS_PROVIDERS, limitsQuery, refreshLimits } from "./limits";

const calls = vi.hoisted(() => ({
  readLimits: vi.fn(),
  listeners: new Set<(change: SharedReadChanged) => void>(),
}));
vi.mock("$lib/api", () => ({
  readLimits: calls.readLimits,
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

function useEveryProvider(pollMinutes: 5 | 30) {
  return LIMITS_PROVIDERS.map((provider) => useQuery(limitsQuery(provider, pollMinutes)));
}

describe("limits refresh policy", () => {
  it("checks the shared cache every five minutes even with a slower default", async () => {
    vi.useFakeTimers();
    calls.listeners.clear();
    calls.readLimits.mockReset().mockResolvedValue([]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const hook = renderHook(() => useEveryProvider(30), { wrapper: windowWith(client) });
    try {
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(calls.readLimits.mock.calls).toEqual([["claude", false], ["codex", false], ["muse", false]]);
      calls.readLimits.mockClear();
      await act(() => vi.advanceTimersByTimeAsync(299_000));
      expect(calls.readLimits).not.toHaveBeenCalled();
      await act(() => vi.advanceTimersByTimeAsync(1_000));
      expect(calls.readLimits.mock.calls).toEqual([["claude", false], ["codex", false], ["muse", false]]);
    } finally {
      hook.unmount();
      client.clear();
      vi.useRealTimers();
    }
  });
});

function reading(provider: AgentId, windows: LimitWindow[] = [WEEKLY]): ProviderLimits[] {
  return [{ provider, status: "ok", account: { id: `${provider}-1`, label: "you@example.com" }, currentAccount: true, windows }];
}
const WEEKLY: LimitWindow = { id: "weekly", label: "Weekly · all models", kind: "weekly", usedPercent: 12, observedAt: "2026-08-17T20:00:00Z" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => { resolve = settle; });
  return { promise, resolve };
}

describe("refreshLimits", () => {
  it("makes a refresh asked during a first read once that read answers, and leaves no force behind for an announcement", async () => {
    calls.listeners.clear();
    const first = deferred<ProviderLimits[]>();
    let claudeReads = 0;
    calls.readLimits.mockReset().mockImplementation((provider: AgentId) =>
      provider === "claude" && ++claudeReads === 1 ? first.promise : Promise.resolve(reading(provider)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderHook(() => useEveryProvider(5), { wrapper: windowWith(client) });
    await waitFor(() => expect(calls.readLimits).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(calls.listeners.size).toBe(1));

    let refreshed: Promise<void> | undefined;
    act(() => { refreshed = refreshLimits(client); });
    await act(async () => first.resolve(reading("claude")));
    await act(async () => { await refreshed; });
    const claude = () => calls.readLimits.mock.calls.filter(([provider]) => provider === "claude");
    expect(claude()).toEqual([["claude", false], ["claude", true]]);

    for (const listener of calls.listeners) listener({ source: "limits:claude" });
    await waitFor(() => expect(claude()).toEqual([["claude", false], ["claude", true], ["claude", false]]));
    client.clear();
  });

  it("does not retain forced reads when no Limits screen is mounted", async () => {
    calls.readLimits.mockReset().mockImplementation((provider: AgentId) => Promise.resolve(reading(provider)));
    const client = new QueryClient({defaultOptions: {queries: {retry: false}}});
    await refreshLimits(client);
    expect(calls.readLimits.mock.calls.map(call => call[1])).toEqual([true, true, true]);
    calls.readLimits.mockClear();
    await client.refetchQueries({queryKey: ["limits"], type: "all"});
    expect(calls.readLimits.mock.calls.map(call => call[1])).toEqual([false, false, false]);
    client.clear();
  });

  it("keeps explicit reload queued when an invalidation replaces the background check", async () => {
    const first = deferred<ProviderLimits[]>();
    const replacement = deferred<ProviderLimits[]>();
    let reads = 0;
    calls.readLimits.mockReset().mockImplementation((provider: AgentId, force: boolean) => provider === "claude" && !force ? (++reads === 1 ? first.promise : replacement.promise) : Promise.resolve(provider === "claude" ? reading("claude", []) : reading(provider)));
    const client = new QueryClient({defaultOptions: {queries: {retry: false}}});
    const queryKey = ["limits", "claude"];
    client.setQueryData(queryKey, reading("claude"));
    const background = client.fetchQuery({queryKey, staleTime: 0, queryFn: () => calls.readLimits("claude", false)}).catch(() => {});
    const refreshed = refreshLimits(client);
    const invalidated = client.refetchQueries({queryKey, type: "all"});
    await Promise.resolve();
    first.resolve(reading("claude"));
    replacement.resolve(reading("claude"));
    await Promise.all([background, refreshed, invalidated]);
    expect(calls.readLimits).toHaveBeenCalledWith("claude", true);
    expect(client.getQueryData<ProviderLimits[]>(queryKey)?.[0].windows).toEqual([]);
    client.clear();
  });
});
