import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SharedReadChanged } from "$lib/types";
import { limitsRefreshMs, useLimitsProviders } from "./useLimitsProviders";

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

    for (const listener of calls.listeners) listener({ source: "limits:codex", revision: 9 });

    await waitFor(() => expect(calls.readLimits).toHaveBeenCalledTimes(3));
    expect(calls.readLimits).toHaveBeenLastCalledWith("codex", false);
  });
});
