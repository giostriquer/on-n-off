import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { defaultNotchSettings, type NotchChanged, type NotchSnapshot } from "$lib/notchTypes";
import { useNotchState } from "./useNotchState";

const calls = vi.hoisted(() => ({
  read: vi.fn(),
  save: vi.fn(),
  listeners: new Set<(change: NotchChanged) => void>(),
}));
vi.mock("$lib/api", () => ({
  readNotchState: calls.read,
  saveNotchSettings: calls.save,
  onNotchChanged: async (listener: (change: NotchChanged) => void) => {
    calls.listeners.add(listener);
    return () => calls.listeners.delete(listener);
  },
}));

it.each(["save", "event"] as const)(
  "does not let a pending read overwrite a newer %s",
  async (source) => {
    const old: NotchSnapshot = {
      revision: 1,
      supported: true,
      settings: defaultNotchSettings({ enabled: true, displayId: "old-display", providers: ["claude", "codex"] }),
      displays: [],
      error: null,
    };
    const saved: NotchSnapshot = {
      ...old,
      revision: 2,
      settings: defaultNotchSettings({ enabled: true, displayId: "new-display", edge: "left", providers: ["claude", "codex"] }),
    };
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    client.setQueryData(["side-notch"], old);
    let finishRead!: (snapshot: NotchSnapshot) => void;
    calls.read.mockReset().mockImplementation(
      () =>
        new Promise<NotchSnapshot>((resolve) => {
          finishRead = resolve;
        }),
    );
    calls.save.mockReset().mockResolvedValue(saved);
    calls.listeners.clear();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const hook = renderHook(() => useNotchState(), { wrapper });
    let pending!: Promise<void>;
    act(() => {
      pending = client.refetchQueries({ queryKey: ["side-notch"] });
    });
    await waitFor(() => expect(calls.read).toHaveBeenCalledTimes(1));
    await act(async () => {
      if (source === "save") await hook.result.current.save(saved.settings);
      else
        for (const listener of calls.listeners)
          listener({ snapshot: saved });
    });
    expect(
      client.getQueryData<NotchSnapshot>(["side-notch"])?.settings,
    ).toEqual(saved.settings);
    await act(async () => {
      finishRead(old);
      await pending;
    });
    expect(
      client.getQueryData<NotchSnapshot>(["side-notch"])?.settings,
    ).toEqual(saved.settings);
    hook.unmount();
    client.clear();
  },
);

it("does not let an older queued event overwrite a newer save", async () => {
  const old = {
    revision: 1,
    supported: true,
    settings: defaultNotchSettings({ enabled: true, displayId: "old-display", providers: ["claude", "codex"] }),
    displays: [],
    error: null,
  } satisfies NotchSnapshot;
  const saved = {
    ...old,
    revision: 2,
    settings: defaultNotchSettings({ enabled: true, displayId: "new-display", edge: "left", providers: ["claude", "codex"] }),
  } satisfies NotchSnapshot;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(["side-notch"], old);
  calls.read.mockReset().mockResolvedValue(old);
  calls.save.mockReset().mockResolvedValue(saved);
  calls.listeners.clear();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useNotchState(), { wrapper });
  await waitFor(() => expect(calls.listeners.size).toBe(1));

  await act(async () => {
    await hook.result.current.save(saved.settings);
  });
  await act(async () => {
    for (const listener of calls.listeners)
      listener({ snapshot: old });
    await Promise.resolve();
  });

  expect(client.getQueryData<NotchSnapshot>(["side-notch"])).toEqual(saved);
  hook.unmount();
  client.clear();
});
