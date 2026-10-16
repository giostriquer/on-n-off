import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { refreshLimits } from "$lib/sharedReads/limits";
import { useLimitsProviders } from "./useLimitsProviders";

const readLimits = vi.hoisted(() => vi.fn());
vi.mock("$lib/api", () => ({ readLimits }));

it("shows every provider Refresh forces, and no other", async () => {
  readLimits.mockReset().mockResolvedValue([]);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  const hook = renderHook(() => useLimitsProviders(5), { wrapper });
  await waitFor(() => expect(hook.result.current.loading).toBe(false));
  readLimits.mockClear();

  await refreshLimits(client);

  const forced = readLimits.mock.calls.filter(([, force]) => force).map(([provider]) => provider);
  expect(hook.result.current.providers.map(({ provider }) => provider)).toEqual(forced);
  hook.unmount();
  client.clear();
});
