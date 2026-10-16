import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { SharedReadChanged, SharedReadSource } from "$lib/types";
import { SharedReads } from "./announcements";
import { githubPrsQuery, refreshGithubPrs } from "./githubPrs";
import { limitsQuery, refreshLimits } from "./limits";

const calls = vi.hoisted(() => ({
  readLimits: vi.fn(),
  readGithubPrs: vi.fn(),
  listeners: new Set<(change: SharedReadChanged) => void>(),
}));
vi.mock("$lib/api", () => ({
  readLimits: calls.readLimits,
  readGithubPrs: calls.readGithubPrs,
  onSharedReadChanged: async (listener: (change: SharedReadChanged) => void) => {
    calls.listeners.add(listener);
    return () => calls.listeners.delete(listener);
  },
}));

type ForcedSource = {
  name: string;
  source: SharedReadSource;
  show: () => unknown;
  refresh: (client: QueryClient) => Promise<void>;
  read: ReturnType<typeof vi.fn>;
  shown: (call: unknown[]) => boolean;
  answer: unknown;
  unforced: unknown[];
  forced: unknown[];
};

const FORCED_SOURCES: ForcedSource[] = [
  {
    name: "Claude's limits",
    source: "limits:claude",
    show: () => useQuery(limitsQuery("claude", 5)),
    refresh: refreshLimits,
    read: calls.readLimits,
    shown: ([provider]) => provider === "claude",
    answer: [],
    unforced: ["claude", false],
    forced: ["claude", true],
  },
  {
    name: "the pull requests",
    source: "github:prs",
    show: () => useQuery(githubPrsQuery(60)),
    refresh: refreshGithubPrs,
    read: calls.readGithubPrs,
    shown: () => true,
    answer: { status: "ok" },
    unforced: [false],
    forced: [true],
  },
];

it.each(FORCED_SOURCES)(
  "$name: a refresh asked during the first read is made once that read answers, and leaves no force behind for an announcement",
  async ({ source, show, refresh, read, shown, answer, unforced, forced }) => {
    calls.listeners.clear();
    let answerFirst!: (value: unknown) => void;
    let waiting = true;
    read.mockReset().mockImplementation((...args: unknown[]) => {
      if (!shown(args) || !waiting) return Promise.resolve(answer);
      waiting = false;
      return new Promise((resolve) => { answerFirst = resolve; });
    });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}><SharedReads />{children}</QueryClientProvider>
    );
    const hook = renderHook(show, { wrapper });
    const reads = () => read.mock.calls.filter(shown);
    await waitFor(() => expect(reads()).toEqual([unforced]));
    await waitFor(() => expect(calls.listeners.size).toBe(1));

    let refreshed: Promise<void> | undefined;
    act(() => { refreshed = refresh(client); });
    await act(async () => answerFirst(answer));
    await act(async () => { await refreshed; });
    expect(reads()).toEqual([unforced, forced]);

    for (const listener of calls.listeners) listener({ source });
    await waitFor(() => expect(reads()).toEqual([unforced, forced, unforced]));
    hook.unmount();
    client.clear();
  },
);
