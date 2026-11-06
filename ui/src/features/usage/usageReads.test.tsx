import type { ReactNode } from "react";
import { QueryClientProvider, useQuery, type QueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMainWindowQueryClient } from "$lib/queryClients";
import type { UsageHistoryStatus } from "$lib/usageTypes";
import { UsageFocusReads } from "./usageFocusReads";
import {
  clearFoldedUsage,
  foldedUsageStatusQuery,
  refreshUsageWindow,
  usageWindowQuery,
} from "./usageReads";

const calls = vi.hoisted(() => ({
  usageSummary: vi.fn<(input: SummaryInput) => Promise<unknown>>(),
  usageHistoryStatus: vi.fn(),
  clearUsageHistory: vi.fn(),
  focusHandlers: new Set<() => void>(),
}));
vi.mock("$lib/api", () => ({
  usageSummary: calls.usageSummary,
  usageHistoryStatus: calls.usageHistoryStatus,
  clearUsageHistory: calls.clearUsageHistory,
  onMainWindowFocused: (handler: () => void) => {
    calls.focusHandlers.add(handler);
    return Promise.resolve(() => calls.focusHandlers.delete(handler));
  },
}));

type SummaryInput = { sinceDay: string; untilDay: string; force: boolean };

const KEPT: UsageHistoryStatus = { state: "kept", keptSince: "2026-08-07T12:00:00.000Z", bytes: 2048 };
const EMPTY: UsageHistoryStatus = { state: "empty", bytes: 0 };

const spanDays = ({ sinceDay, untilDay }: SummaryInput) =>
  (Date.parse(untilDay) - Date.parse(sinceDay)) / 86_400_000 + 1;
const reads = () => calls.usageSummary.mock.calls.map(([input]) => [spanDays(input), input.force]);
const answer = (label: string) => ({ label });
const shown = (data: unknown) => (data as { summary: { label: string } } | undefined)?.summary.label;

let clients: QueryClient[] = [];

function mainWindowClient(): QueryClient {
  const client = createMainWindowQueryClient();
  client.mount();
  clients.push(client);
  return client;
}

function wrapperFor(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-08-15T15:00:00.000Z"));
  calls.usageSummary.mockReset();
  calls.usageHistoryStatus.mockReset().mockResolvedValue(KEPT);
  calls.clearUsageHistory.mockReset().mockResolvedValue(EMPTY);
  calls.focusHandlers.clear();
  clients = [];
});

afterEach(() => {
  for (const client of clients) {
    client.unmount();
    client.clear();
  }
  vi.useRealTimers();
});

it("with no screen showing the window, a refresh reads it forced every time it is asked, and the read it leaves behind is unforced", async () => {
  calls.usageSummary.mockResolvedValue(answer("figures"));
  const client = mainWindowClient();

  await act(async () => { await refreshUsageWindow(client, 30); });
  expect(reads()).toEqual([[30, true]]);
  await act(async () => { await client.refetchQueries({ queryKey: usageWindowQuery(30).queryKey, type: "all" }); });
  expect(reads()).toEqual([[30, true], [30, false]]);
  await act(async () => { await refreshUsageWindow(client, 30); });
  expect(reads()).toEqual([[30, true], [30, false], [30, true]]);
});

it("a refresh asked during a read of the window waits for it, then reads forced once", async () => {
  let answerFirst!: (value: unknown) => void;
  calls.usageSummary
    .mockImplementationOnce(() => new Promise((resolve) => { answerFirst = resolve; }))
    .mockResolvedValue(answer("refreshed"));
  const client = mainWindowClient();
  const hook = renderHook(() => useQuery(usageWindowQuery(30)), { wrapper: wrapperFor(client) });
  await waitFor(() => expect(reads()).toEqual([[30, false]]));

  let refreshed: Promise<void> | undefined;
  act(() => { refreshed = refreshUsageWindow(client, 30); });
  expect(reads()).toEqual([[30, false]]);
  await act(async () => answerFirst(answer("first")));
  await act(async () => { await refreshed; });

  expect(reads()).toEqual([[30, false], [30, true]]);
  await waitFor(() => expect(shown(hook.result.current.data)).toBe("refreshed"));
  hook.unmount();
});

it("keeps one entry per window: a refresh replaces what every view of that window shows and no other window", async () => {
  calls.usageSummary.mockImplementation((input: SummaryInput) =>
    Promise.resolve(answer(input.force ? `forced ${spanDays(input)}` : `read ${spanDays(input)}`)),
  );
  const onRead = vi.fn();
  const client = mainWindowClient();
  const hook = renderHook(
    () => [useQuery(usageWindowQuery(30, onRead)), useQuery(usageWindowQuery(30)), useQuery(usageWindowQuery(7))],
    { wrapper: wrapperFor(client) },
  );
  await waitFor(() => expect(hook.result.current.map((query) => shown(query.data))).toEqual(["read 30", "read 30", "read 7"]));
  expect(reads()).toEqual([[30, false], [7, false]]);
  expect(onRead).toHaveBeenCalledTimes(1);

  await act(async () => { await refreshUsageWindow(client, 30); });

  await waitFor(() => expect(hook.result.current.map((query) => shown(query.data))).toEqual(["forced 30", "forced 30", "read 7"]));
  expect(reads()).toEqual([[30, false], [7, false], [30, true]]);

  await act(async () => { await refreshUsageWindow(client, 7); });

  await waitFor(() => expect(hook.result.current.map((query) => shown(query.data))).toEqual(["forced 30", "forced 30", "forced 7"]));
  expect(reads()).toEqual([[30, false], [7, false], [30, true], [7, true]]);
  hook.unmount();
});

it("a refresh never waits for a read of another window", async () => {
  let answerWeek!: (value: unknown) => void;
  calls.usageSummary
    .mockImplementationOnce(() => new Promise((resolve) => { answerWeek = resolve; }))
    .mockResolvedValue(answer("forced 30"));
  const client = mainWindowClient();
  const hook = renderHook(() => useQuery(usageWindowQuery(7)), { wrapper: wrapperFor(client) });
  await waitFor(() => expect(reads()).toEqual([[7, false]]));

  let refreshed: Promise<void> | undefined;
  act(() => { refreshed = refreshUsageWindow(client, 30); });

  await waitFor(() => expect(reads()).toEqual([[7, false], [30, true]]));
  await act(async () => { await refreshed; });
  expect(shown(client.getQueryData(usageWindowQuery(30).queryKey))).toBe("forced 30");
  await act(async () => answerWeek(answer("read 7")));
  await waitFor(() => expect(shown(hook.result.current.data)).toBe("read 7"));
  hook.unmount();
});

it("leaves a failed refresh's error on the window and the figures it already had, without rejecting", async () => {
  calls.usageSummary.mockResolvedValueOnce(answer("first")).mockRejectedValueOnce(new Error("scan failed"));
  const client = mainWindowClient();
  const hook = renderHook(
    () => {
      const { data, error } = useQuery(usageWindowQuery(30));
      return { data, error };
    },
    { wrapper: wrapperFor(client) },
  );
  await waitFor(() => expect(shown(hook.result.current.data)).toBe("first"));

  await act(async () => { await expect(refreshUsageWindow(client, 30)).resolves.toBeUndefined(); });

  await waitFor(() => expect(hook.result.current.error).toEqual(new Error("scan failed")));
  expect(shown(hook.result.current.data)).toBe("first");
  expect(reads()).toEqual([[30, false], [30, true]]);
  hook.unmount();
});

it("clearing the folded usage shows the status the clear returned and reads the shown windows again, unforced", async () => {
  calls.usageSummary.mockResolvedValueOnce(answer("before")).mockResolvedValueOnce(answer("after"));
  const client = mainWindowClient();
  const hook = renderHook(() => [useQuery(foldedUsageStatusQuery()), useQuery(usageWindowQuery(30))] as const, {
    wrapper: wrapperFor(client),
  });
  await waitFor(() => expect(hook.result.current[0].data).toEqual(KEPT));
  await waitFor(() => expect(shown(hook.result.current[1].data)).toBe("before"));

  let cleared: UsageHistoryStatus | undefined;
  await act(async () => { cleared = await clearFoldedUsage(client); });

  expect(cleared).toEqual(EMPTY);
  expect(client.getQueryData(foldedUsageStatusQuery().queryKey)).toEqual(EMPTY);
  await waitFor(() => expect(hook.result.current[0].data).toEqual(EMPTY));
  await waitFor(() => expect(shown(hook.result.current[1].data)).toBe("after"));
  expect(reads()).toEqual([[30, false], [30, false]]);
  expect(calls.usageHistoryStatus).toHaveBeenCalledTimes(1);
  hook.unmount();
});

it("re-reads the stale Usage windows when the main window gains focus, never the folded-usage status", async () => {
  calls.usageSummary.mockResolvedValue(answer("figures"));
  const client = mainWindowClient();
  const hook = renderHook(() => [useQuery(foldedUsageStatusQuery()), useQuery(usageWindowQuery(30))] as const, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>
        <UsageFocusReads />
        {children}
      </QueryClientProvider>
    ),
  });
  await waitFor(() => expect(calls.focusHandlers.size).toBe(1));
  await waitFor(() => expect(hook.result.current[0].data).toEqual(KEPT));
  await waitFor(() => expect(reads()).toEqual([[30, false]]));

  vi.setSystemTime(new Date("2026-08-15T15:00:31.000Z"));
  await act(async () => {
    for (const handler of calls.focusHandlers) handler();
  });

  await waitFor(() => expect(reads()).toEqual([[30, false], [30, false]]));
  expect(calls.usageHistoryStatus).toHaveBeenCalledTimes(1);
  hook.unmount();
});
