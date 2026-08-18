import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { Root } from "./Root";

vi.mock("$lib/api", () => ({
  readLimits: vi.fn((provider: "claude" | "codex") =>
    Promise.resolve([
      {
        provider,
        status: "signedOut",
        message: `No ${provider} login`,
        live: true,
        windows: [],
        fetchedAt: "2026-08-18T12:00:00Z",
      },
    ]),
  ),
  onLimitsPopoverOpened: vi.fn(() => Promise.resolve(() => undefined)),
  hideLimitsPopover: vi.fn(() => Promise.resolve()),
  openLimitsWindow: vi.fn(() => Promise.resolve()),
  quitApp: vi.fn(() => Promise.resolve()),
}));

it("selects the lightweight Limits surface from the window query", () => {
  render(<Root search="?surface=limits-popover" />);

  expect(screen.getByRole("heading", { name: "Subscription limits" })).toBeTruthy();
});
