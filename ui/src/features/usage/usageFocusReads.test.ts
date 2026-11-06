import { QueryObserver, type QueryClient } from "@tanstack/react-query";
import { act, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMainWindowQueryClient } from "$lib/queryClients";
import { connectUsageFocusReads } from "./usageFocusReads";

const focusHandlers = vi.hoisted(() => new Set<() => void>());
const stopListening = vi.hoisted(() => vi.fn());
const onMainWindowFocused = vi.hoisted(() =>
  vi.fn((handler: () => void) => {
    focusHandlers.add(handler);
    return Promise.resolve(() => {
      focusHandlers.delete(handler);
      stopListening();
    });
  }),
);

vi.mock("$lib/api", () => ({ onMainWindowFocused }));

const focusMainWindow = () => {
  for (const handler of focusHandlers) handler();
};

function observe(client: QueryClient, queryKey: readonly unknown[], queryFn: () => Promise<unknown>) {
  const observer = new QueryObserver(client, { queryKey, queryFn, refetchOnWindowFocus: true });
  return observer.subscribe(() => undefined);
}

let unsubscribers: (() => void)[] = [];

function mountedClient(): QueryClient {
  const client = createMainWindowQueryClient();
  client.mount();
  unsubscribers.push(() => client.unmount());
  return client;
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-08-15T15:00:00.000Z"));
  focusHandlers.clear();
  stopListening.mockClear();
  onMainWindowFocused.mockClear();
  unsubscribers = [];
});

afterEach(() => {
  for (const unsubscribe of unsubscribers) unsubscribe();
  vi.useRealTimers();
});

it("re-reads only the stale usage queries when the main window gains focus, never the window's other reads", async () => {
  const client = mountedClient();
  const usageRead = vi.fn(() => Promise.resolve("usage"));
  const limitsRead = vi.fn(() => Promise.resolve("limits"));
  unsubscribers.push(observe(client, ["usage", 30, 0], usageRead), observe(client, ["limits", "claude"], limitsRead));
  const disconnect = connectUsageFocusReads(client);
  await waitFor(() => expect(focusHandlers.size).toBe(1));
  await waitFor(() => expect(usageRead).toHaveBeenCalledTimes(1));
  expect(limitsRead).toHaveBeenCalledTimes(1);

  vi.setSystemTime(new Date("2026-08-15T15:00:29.000Z"));
  await act(async () => focusMainWindow());
  expect(usageRead).toHaveBeenCalledTimes(1);

  vi.setSystemTime(new Date("2026-08-15T15:00:31.000Z"));
  await act(async () => focusMainWindow());
  await waitFor(() => expect(usageRead).toHaveBeenCalledTimes(2));
  expect(limitsRead).toHaveBeenCalledTimes(1);
  disconnect();
});

it("leaves cached usage windows that no view shows alone when the main window gains focus", async () => {
  const client = mountedClient();
  const ninetyDayRead = vi.fn(() => Promise.resolve("90 days"));
  const leftWindow = observe(client, ["usage", 90, 0], ninetyDayRead);
  await waitFor(() => expect(ninetyDayRead).toHaveBeenCalledTimes(1));
  leftWindow();
  const shownRead = vi.fn(() => Promise.resolve("30 days"));
  unsubscribers.push(observe(client, ["usage", 30, 0], shownRead));
  const disconnect = connectUsageFocusReads(client);
  await waitFor(() => expect(focusHandlers.size).toBe(1));
  await waitFor(() => expect(shownRead).toHaveBeenCalledTimes(1));

  vi.setSystemTime(new Date("2026-08-15T15:00:31.000Z"));
  await act(async () => focusMainWindow());

  await waitFor(() => expect(shownRead).toHaveBeenCalledTimes(2));
  expect(ninetyDayRead).toHaveBeenCalledTimes(1);
  expect(client.getQueryData(["usage", 90, 0])).toBe("90 days");
  disconnect();
});

it("joins a usage re-read already in flight instead of starting a second scan", async () => {
  const client = mountedClient();
  let finish!: (value: string) => void;
  const usageRead = vi
    .fn<() => Promise<string>>()
    .mockResolvedValueOnce("first")
    .mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
  unsubscribers.push(observe(client, ["usage", 30, 0], usageRead));
  const disconnect = connectUsageFocusReads(client);
  await waitFor(() => expect(focusHandlers.size).toBe(1));
  await waitFor(() => expect(client.getQueryData(["usage", 30, 0])).toBe("first"));

  vi.setSystemTime(new Date("2026-08-15T15:00:31.000Z"));
  await act(async () => document.dispatchEvent(new Event("visibilitychange", { bubbles: true })));
  expect(usageRead).toHaveBeenCalledTimes(2);

  await act(async () => focusMainWindow());
  expect(usageRead).toHaveBeenCalledTimes(2);

  await act(async () => finish("second"));
  await waitFor(() => expect(client.getQueryData(["usage", 30, 0])).toBe("second"));
  expect(usageRead).toHaveBeenCalledTimes(2);
  disconnect();
});

it("stops listening when disconnected, even before the listener has finished registering", async () => {
  const client = mountedClient();
  const disconnect = connectUsageFocusReads(client);
  disconnect();

  await waitFor(() => expect(stopListening).toHaveBeenCalledTimes(1));
  expect(focusHandlers.size).toBe(0);
});

it("leaves the window usable when the focus listener cannot be registered", async () => {
  onMainWindowFocused.mockImplementationOnce(() => Promise.reject(new Error("event plugin unavailable")));
  const client = mountedClient();

  const disconnect = connectUsageFocusReads(client);
  await new Promise((resolve) => setTimeout(resolve, 0));

  expect(() => disconnect()).not.toThrow();
  expect(stopListening).not.toHaveBeenCalled();
});
