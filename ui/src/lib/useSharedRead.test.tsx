import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { SharedReadChanged } from "$lib/types";
import { useSharedRead } from "./useSharedRead";

const calls = vi.hoisted(() => ({
  listeners: new Set<(change: SharedReadChanged) => void>(),
  unlisten: vi.fn(),
}));
vi.mock("$lib/api", () => ({
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

it("refetches only the query whose shared read was replaced, and stops listening on unmount", async () => {
  calls.listeners.clear();
  calls.unlisten.mockReset();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(client, "invalidateQueries").mockResolvedValue();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useSharedRead("limits:codex"), { wrapper });
  await waitFor(() => expect(calls.listeners.size).toBe(1));

  announce("limits:claude");
  announce("github:prs");
  expect(invalidate).not.toHaveBeenCalled();

  announce("limits:codex");
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ["limits", "codex"] });

  hook.unmount();
  expect(calls.unlisten).toHaveBeenCalledTimes(1);
  expect(calls.listeners.size).toBe(0);
});

it.each([
  ["limits:claude", ["limits", "claude"]],
  ["limits:codex", ["limits", "codex"]],
  ["github:prs", ["github", "prs"]],
] as const)("refetches the query %s actually backs", async (source, queryKey) => {
  calls.listeners.clear();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(client, "invalidateQueries").mockResolvedValue();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  renderHook(() => useSharedRead(source), { wrapper });
  await waitFor(() => expect(calls.listeners.size).toBe(1));

  announce(source);
  expect(invalidate).toHaveBeenCalledWith({ queryKey: [...queryKey] });
});
