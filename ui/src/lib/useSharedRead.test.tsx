import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { SharedReadChanged } from "$lib/types";
import { usePendingResetSpends } from "./usePendingResetSpends";
import { connectSharedReads, SharedReads } from "./useSharedRead";

const calls = vi.hoisted(() => ({
  listeners: new Set<(change: SharedReadChanged) => void>(),
  unlisten: vi.fn(),
  pendingResetSpends: vi.fn(),
}));
vi.mock("$lib/api", () => ({
  pendingResetSpends: calls.pendingResetSpends,
  onSharedReadChanged: async (listener: (change: SharedReadChanged) => void) => {
    calls.listeners.add(listener);
    return () => {
      calls.unlisten();
      calls.listeners.delete(listener);
    };
  },
}));

function announce(source: SharedReadChanged["source"]) {
  for (const listener of calls.listeners) listener({ source });
}

it("refetches only the query whose shared read was replaced, and stops listening once disconnected", async () => {
  calls.listeners.clear();
  calls.unlisten.mockReset();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(client, "invalidateQueries").mockResolvedValue();
  const disconnect = connectSharedReads(client);
  await waitFor(() => expect(calls.listeners.size).toBe(1));

  announce("limits:claude");
  expect(invalidate.mock.calls).toEqual([[{ queryKey: ["limits", "claude"] }]]);
  invalidate.mockClear();
  announce("github:prs");
  expect(invalidate.mock.calls).toEqual([[{ queryKey: ["github", "prs"] }]]);

  disconnect();
  expect(calls.unlisten).toHaveBeenCalledTimes(1);
  expect(calls.listeners.size).toBe(0);
});

it("stops listening when disconnected before the listener is in place", async () => {
  calls.listeners.clear();
  calls.unlisten.mockReset();
  const disconnect = connectSharedReads(new QueryClient());
  disconnect();
  await waitFor(() => expect(calls.unlisten).toHaveBeenCalledTimes(1));
  expect(calls.listeners.size).toBe(0);
});

it.each([
  ["limits:claude", ["limits", "claude"]],
  ["limits:codex", ["limits", "codex"]],
  ["github:prs", ["github", "prs"]],
  ["limits:reset-spends", ["reset-spends"]],
] as const)("refetches the query %s actually backs", async (source, queryKey) => {
  calls.listeners.clear();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(client, "invalidateQueries").mockResolvedValue();
  const disconnect = connectSharedReads(client);
  await waitFor(() => expect(calls.listeners.size).toBe(1));

  announce(source);
  expect(invalidate).toHaveBeenCalledWith({ queryKey: [...queryKey] });
  disconnect();
});

it("rereads a cached Codex subscription date when the accounts change", async () => {
  calls.listeners.clear();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(["accounts", "codex"], { profiles: [] });
  client.setQueryData(["subscription", "codex", "profile:a"], null);
  const disconnect = connectSharedReads(client);
  await waitFor(() => expect(calls.listeners.size).toBe(1));
  announce("accounts");
  expect(client.getQueryState(["accounts", "codex"])?.isInvalidated).toBe(true);
  expect(client.getQueryState(["subscription", "codex", "profile:a"])?.isInvalidated).toBe(true);
  disconnect(); client.clear();
});

function Spends() {
  usePendingResetSpends();
  return null;
}

function renderWindow(children: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 30_000 } } });
  const tree = (next: ReactNode) => <QueryClientProvider client={client}><SharedReads />{next}</QueryClientProvider>;
  const view = render(tree(children));
  return { ...view, client, show: (next: ReactNode) => view.rerender(tree(next)) };
}

it("re-reads a shared read once per announcement, however many surfaces in the window show it", async () => {
  calls.listeners.clear();
  calls.pendingResetSpends.mockReset().mockResolvedValue([]);
  const view = renderWindow(<><Spends /><Spends /></>);
  await waitFor(() => expect(calls.pendingResetSpends).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(calls.listeners.size).toBeGreaterThan(0));

  announce("limits:reset-spends");

  await waitFor(() => expect(calls.pendingResetSpends).toHaveBeenCalledTimes(2));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(calls.pendingResetSpends).toHaveBeenCalledTimes(2);
  view.unmount(); view.client.clear();
});

it("marks a shared read no surface shows as replaced, so the next surface to show it reads it again", async () => {
  calls.listeners.clear();
  calls.pendingResetSpends.mockReset().mockResolvedValue([]);
  const view = renderWindow(<Spends />);
  await waitFor(() => expect(calls.pendingResetSpends).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(calls.listeners.size).toBe(1));
  view.show(null);

  announce("limits:reset-spends");
  view.show(<Spends />);

  await waitFor(() => expect(calls.pendingResetSpends).toHaveBeenCalledTimes(2));
  view.unmount(); view.client.clear();
});
