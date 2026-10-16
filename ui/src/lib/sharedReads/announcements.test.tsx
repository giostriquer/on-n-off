import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider, useQuery, useQueryClient } from "@tanstack/react-query";
import { act, render, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SharedReadChanged, SharedReadSource } from "$lib/types";
import { accountListQuery, accountPreferenceQuery, codexSubscriptionQuery } from "./accounts";
import { connectSharedReads, SharedReads } from "./announcements";
import { githubPrsQuery, refreshGithubPrs } from "./githubPrs";
import { limitsQuery, refreshLimits } from "./limits";
import { pendingResetSpendsQuery } from "./resetSpends";

const calls = vi.hoisted(() => ({
  listeners: new Set<(change: SharedReadChanged) => void>(),
  unlisten: vi.fn(),
  reads: {
    readLimits: vi.fn(),
    readGithubPrs: vi.fn(),
    pendingResetSpends: vi.fn(),
    readAccounts: vi.fn(),
    readAccountPreferences: vi.fn(),
    readCodexSubscription: vi.fn(),
  },
}));
vi.mock("$lib/api", () => ({
  ...calls.reads,
  onSharedReadChanged: async (listener: (change: SharedReadChanged) => void) => {
    calls.listeners.add(listener);
    return () => {
      calls.unlisten();
      calls.listeners.delete(listener);
    };
  },
}));

type Reads = Partial<Record<keyof typeof calls.reads, unknown[][]>>;

function resetReads() {
  calls.listeners.clear();
  calls.unlisten.mockReset();
  calls.reads.readLimits.mockReset().mockResolvedValue([]);
  calls.reads.readGithubPrs.mockReset().mockResolvedValue({ status: "ok" });
  calls.reads.pendingResetSpends.mockReset().mockResolvedValue([]);
  calls.reads.readAccounts.mockReset().mockResolvedValue({ profiles: [], nativeAccount: null, recoveryRequired: false, notice: null });
  calls.reads.readAccountPreferences.mockReset().mockResolvedValue(false);
  calls.reads.readCodexSubscription.mockReset().mockResolvedValue(null);
}

function readsMade(): Reads {
  const made: Reads = {};
  for (const [name, read] of Object.entries(calls.reads) as [keyof typeof calls.reads, ReturnType<typeof vi.fn>][]) {
    if (read.mock.calls.length) made[name] = read.mock.calls;
  }
  return made;
}

function clearReads() {
  for (const read of Object.values(calls.reads)) read.mockClear();
}

function announce(source: SharedReadSource) {
  for (const listener of calls.listeners) listener({ source });
}

function windowWith(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}><SharedReads />{children}</QueryClientProvider>
  );
}

function useLimitsSurface(provider: "claude" | "codex" | "muse") {
  const client = useQueryClient();
  useQuery(limitsQuery(provider, 5));
  return () => refreshLimits(client);
}

function useGithubSurface() {
  const client = useQueryClient();
  useQuery(githubPrsQuery(60));
  return () => refreshGithubPrs(client);
}

function useAccountsSurface() {
  useQuery(accountListQuery("claude"));
  useQuery(accountListQuery("codex"));
  useQuery(accountPreferenceQuery());
  useQuery(codexSubscriptionQuery("acct-work"));
}

const SURFACES: Record<SharedReadSource, { show: () => (() => unknown) | void; answer: Reads; forced?: Reads }> = {
  "limits:claude": {
    show: () => useLimitsSurface("claude"),
    forced: { readLimits: [["claude", true], ["codex", true], ["muse", true]] },
    answer: { readLimits: [["claude", false]] },
  },
  "limits:codex": {
    show: () => useLimitsSurface("codex"),
    forced: { readLimits: [["claude", true], ["codex", true], ["muse", true]] },
    answer: { readLimits: [["codex", false]] },
  },
  "limits:muse": {
    show: () => useLimitsSurface("muse"),
    forced: { readLimits: [["claude", true], ["codex", true], ["muse", true]] },
    answer: { readLimits: [["muse", false]] },
  },
  "github:prs": {
    show: useGithubSurface,
    forced: { readGithubPrs: [[true]] },
    answer: { readGithubPrs: [[false]] },
  },
  "limits:reset-spends": {
    show: () => { useQuery(pendingResetSpendsQuery()); },
    answer: { pendingResetSpends: [[]] },
  },
  accounts: {
    show: useAccountsSurface,
    answer: {
      readAccounts: [["claude"], ["codex"]],
      readAccountPreferences: [[]],
      readCodexSubscription: [["acct-work"]],
    },
  },
};

describe("a shared read's announcement", () => {
  it.each(Object.keys(SURFACES) as SharedReadSource[])(
    "%s: every surface showing it reads it again once, unforced, after any forced read, and nothing else is read",
    async (source) => {
      resetReads();
      const { show, answer, forced } = SURFACES[source];
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const hook = renderHook(() => { const force = show(); show(); return force; }, { wrapper: windowWith(client) });
      await waitFor(() => expect(Object.keys(readsMade()).sort()).toEqual(Object.keys(answer).sort()));
      await waitFor(() => expect(calls.listeners.size).toBe(1));

      if (forced) {
        clearReads();
        await act(async () => { await hook.result.current?.(); });
        await waitFor(() => expect(readsMade()).toEqual(forced));
      }
      clearReads();
      await act(async () => announce(source));

      await waitFor(() => expect(readsMade()).toEqual(answer));
      await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
      expect(readsMade()).toEqual(answer);
      hook.unmount(); client.clear();
    },
  );

  it("marks a shared read no surface shows as replaced, so the next surface to show it reads it again", async () => {
    resetReads();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 30_000 } } });
    function Spends() {
      useQuery(pendingResetSpendsQuery());
      return null;
    }
    const tree = (next: ReactNode) => <QueryClientProvider client={client}><SharedReads />{next}</QueryClientProvider>;
    const view = render(tree(<Spends />));
    await waitFor(() => expect(calls.reads.pendingResetSpends).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(calls.listeners.size).toBe(1));
    view.rerender(tree(null));

    announce("limits:reset-spends");
    view.rerender(tree(<Spends />));

    await waitFor(() => expect(calls.reads.pendingResetSpends).toHaveBeenCalledTimes(2));
    view.unmount(); client.clear();
  });
});

describe("the window's listener", () => {
  it("invalidates only the queries of the source announced, and stops listening once disconnected", async () => {
    resetReads();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, "invalidateQueries").mockResolvedValue();
    const disconnect = connectSharedReads(client);
    await waitFor(() => expect(calls.listeners.size).toBe(1));

    announce("limits:claude");
    expect(invalidate.mock.calls).toEqual([[{ queryKey: ["limits", "claude"] }]]);
    invalidate.mockClear();
    announce("accounts");
    expect(invalidate.mock.calls).toEqual([[{ queryKey: ["accounts"] }], [{ queryKey: ["subscription", "codex"] }]]);

    disconnect();
    expect(calls.unlisten).toHaveBeenCalledTimes(1);
    expect(calls.listeners.size).toBe(0);
  });

  it("stops listening when disconnected before the listener is in place", async () => {
    resetReads();
    const disconnect = connectSharedReads(new QueryClient());
    disconnect();
    await waitFor(() => expect(calls.unlisten).toHaveBeenCalledTimes(1));
    expect(calls.listeners.size).toBe(0);
  });

  it("ignores a source it has no queries for", async () => {
    resetReads();
    const client = new QueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries").mockResolvedValue();
    const disconnect = connectSharedReads(client);
    await waitFor(() => expect(calls.listeners.size).toBe(1));

    expect(() => announce("limits:acme" as SharedReadSource)).not.toThrow();
    expect(invalidate).not.toHaveBeenCalled();
    disconnect();
  });
});
