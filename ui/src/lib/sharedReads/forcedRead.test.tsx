import type { ReactNode } from "react";
import { QueryClientProvider, useQuery, type QueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { createMainWindowQueryClient, createPopoverQueryClient } from "$lib/queryClients";
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
  queryKey: readonly unknown[];
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
    queryKey: limitsQuery("claude", 5).queryKey,
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
    queryKey: githubPrsQuery(60).queryKey,
    show: () => useQuery(githubPrsQuery(60)),
    refresh: refreshGithubPrs,
    read: calls.readGithubPrs,
    shown: () => true,
    answer: { status: "ok" },
    unforced: [false],
    forced: [true],
  },
];

const WINDOWS = [
  { window: "main window", client: createMainWindowQueryClient },
  { window: "popover", client: createPopoverQueryClient },
];

const CASES = FORCED_SOURCES.flatMap((forcedSource) => WINDOWS.map((window) => ({ ...forcedSource, ...window })));

it.each(CASES)(
  "$name, in the $window: a refresh asked during the first read is made once that read answers, and leaves no force behind for an announcement",
  async ({ source, show, refresh, read, shown, answer, unforced, forced, client: createClient }) => {
    calls.listeners.clear();
    let answerFirst!: (value: unknown) => void;
    let waiting = true;
    read.mockReset().mockImplementation((...args: unknown[]) => {
      if (!shown(args) || !waiting) return Promise.resolve(answer);
      waiting = false;
      return new Promise((resolve) => { answerFirst = resolve; });
    });
    const client = createClient();
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

it.each(CASES)(
  "$name, in the $window, with no screen showing it: a refresh reads forced every time it is asked, and the read it leaves behind is unforced",
  async ({ queryKey, refresh, read, shown, answer, unforced, forced, client: createClient }) => {
    read.mockReset().mockResolvedValue(answer);
    const client = createClient();
    const reads = () => read.mock.calls.filter(shown);

    await act(async () => { await refresh(client); });
    expect(reads()).toEqual([forced]);
    await act(async () => { await client.refetchQueries({ queryKey, type: "all" }); });
    expect(reads()).toEqual([forced, unforced]);
    await act(async () => { await refresh(client); });
    expect(reads()).toEqual([forced, unforced, forced]);
    client.clear();
  },
);
