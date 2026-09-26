import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId, SharedReadChanged } from "$lib/types";
import { limitsRefreshMs, refreshLimits, useLimitsProviders } from "./useLimitsProviders";

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

describe("limits refresh policy", () => {
  it("converts every supported setting to one refresh interval", () => {
    const fiveMinutes = limitsRefreshMs(5);
    expect(fiveMinutes).toBe(300_000);
    expect(limitsRefreshMs(10)).toBe(600_000);
  });

  it("picks up a read another surface already made, unforced, without waiting out the interval", async () => {
    calls.listeners.clear();
    calls.readLimits.mockReset().mockResolvedValue([]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    renderHook(() => useLimitsProviders(5), { wrapper });
    await waitFor(() => expect(calls.readLimits).toHaveBeenCalledTimes(2));
    expect(calls.readLimits.mock.calls).toEqual([
      ["claude", false],
      ["codex", false],
    ]);

    for (const listener of calls.listeners) listener({ source: "limits:codex" });

    await waitFor(() => expect(calls.readLimits).toHaveBeenCalledTimes(3));
    expect(calls.readLimits).toHaveBeenLastCalledWith("codex", false);
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
  it("does not retain forced reads when no Limits screen is mounted", async () => {
    calls.readLimits.mockReset().mockImplementation((provider: AgentId) => Promise.resolve(reading(provider)));
    const client = new QueryClient({defaultOptions: {queries: {retry: false}}});
    await refreshLimits(client);
    expect(calls.readLimits.mock.calls.map(call => call[1])).toEqual([true, true]);
    calls.readLimits.mockClear();
    await client.refetchQueries({queryKey: ["limits"], type: "all"});
    expect(calls.readLimits.mock.calls.map(call => call[1])).toEqual([false, false]);
    client.clear();
  });

  it("keeps explicit reload queued when an invalidation replaces the background check", async () => {
    const first = deferred<ProviderLimits[]>();
    const replacement = deferred<ProviderLimits[]>();
    let reads = 0;
    calls.readLimits.mockReset().mockImplementation((provider: AgentId, force: boolean) => provider === "claude" && !force ? (++reads === 1 ? first.promise : replacement.promise) : Promise.resolve(provider === "claude" ? reading("claude", []) : reading("codex")));
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
