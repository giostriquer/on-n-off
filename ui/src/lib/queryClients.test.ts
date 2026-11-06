import { QueryObserver } from "@tanstack/react-query";
import { act, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMainWindowQueryClient } from "./queryClients";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-08-15T15:00:00.000Z"));
});

afterEach(() => {
  vi.useRealTimers();
});

it("keeps the main window's reads still when the webview becomes visible unless a read opts in", async () => {
  const client = createMainWindowQueryClient();
  client.mount();
  const read = vi.fn(() => Promise.resolve("figures"));
  const unsubscribe = new QueryObserver(client, { queryKey: ["acme"], queryFn: read }).subscribe(() => undefined);
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));

  vi.setSystemTime(new Date("2026-08-15T15:05:00.000Z"));
  await act(async () => document.dispatchEvent(new Event("visibilitychange", { bubbles: true })));

  expect(read).toHaveBeenCalledTimes(1);
  unsubscribe();
  client.unmount();
});
