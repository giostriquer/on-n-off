import type { ReactNode } from "react";
import { render, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import type { SharedReadChanged } from "$lib/types";
import { useQuery } from "@tanstack/react-query";
import { pendingResetSpendsQuery } from "$lib/sharedReads/resetSpends";
import { App } from "./App";

const announcements = vi.hoisted(() => new Set<(change: SharedReadChanged) => void>());
const pendingResetSpends = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  loadAppSettings: () => Promise.resolve(DEFAULT_APP_SETTINGS),
  pendingResetSpends,
  onSharedReadChanged: (listener: (change: SharedReadChanged) => void) => {
    announcements.add(listener);
    return Promise.resolve(() => announcements.delete(listener));
  },
}));
vi.mock("./router", () => ({ router: {} }));
vi.mock("@tanstack/react-router", () => ({ RouterProvider: () => <Screen /> }));
vi.mock("@/features/session/SessionProvider", () => ({
  SessionProvider: ({ children }: { children: ReactNode }) => children,
  useAgentSession: () => ({ initialProviderReady: true }),
}));
vi.mock("@/features/updater/UpdateProvider", () => ({
  UpdateProvider: ({ children }: { children: ReactNode }) => children,
}));

function Screen() {
  useQuery(pendingResetSpendsQuery());
  return null;
}

it("keeps the main window told: a shared read another surface replaced is read again there", async () => {
  pendingResetSpends.mockResolvedValue([]);
  render(<App />);
  await waitFor(() => expect(pendingResetSpends).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(announcements.size).toBe(1));

  for (const listener of announcements) listener({ source: "limits:reset-spends" });

  await waitFor(() => expect(pendingResetSpends).toHaveBeenCalledTimes(2));
});
