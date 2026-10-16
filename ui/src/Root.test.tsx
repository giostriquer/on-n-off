import { render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import type { SharedReadChanged } from "$lib/types";
import { Root } from "./Root";

const announcements = vi.hoisted(() => new Set<(change: SharedReadChanged) => void>());
const api = vi.hoisted(() => ({
  readLimits: vi.fn((provider: "claude" | "codex" | "muse") =>
    Promise.resolve([
      {
        provider,
        status: "signedOut",
        message: `No ${provider} login`,
        currentAccount: true,
        windows: [],
      },
    ]),
  ),
}));

vi.mock("$lib/api", () => ({
  loadAppSettings: vi.fn(() => Promise.resolve(DEFAULT_APP_SETTINGS)),
  readLimits: api.readLimits,
  onLimitsPopoverOpened: vi.fn(() => Promise.resolve(() => undefined)),
  hideLimitsPopover: vi.fn(() => Promise.resolve()),
  openLimitsWindow: vi.fn(() => Promise.resolve()),
  quitApp: vi.fn(() => Promise.resolve()),
  onSharedReadChanged: (listener: (change: SharedReadChanged) => void) => {
    announcements.add(listener);
    return Promise.resolve(() => announcements.delete(listener));
  },
}));

it("selects the lightweight Limits surface from the window query", async () => {
  render(<Root search="?surface=limits-popover" />);

  expect(await screen.findByRole("heading", { name: "Limits" })).toBeTruthy();
});

it("keeps the popover told: another surface's replacement of a provider's limits is read again there, unforced", async () => {
  render(<Root search="?surface=limits-popover" />);
  await screen.findByRole("heading", { name: "Limits" });
  await waitFor(() => expect(announcements.size).toBe(1));
  api.readLimits.mockClear();

  for (const listener of announcements) listener({ source: "limits:codex" });

  await waitFor(() => expect(api.readLimits.mock.calls).toEqual([["codex", false]]));
});
