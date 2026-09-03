import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { Root } from "./Root";

vi.mock("$lib/api", () => ({
  loadAppSettings: vi.fn(() => Promise.resolve({ limitsPollMinutes: 5 })),
  readLimits: vi.fn((provider: "claude" | "codex") =>
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
  onLimitsPopoverOpened: vi.fn(() => Promise.resolve(() => undefined)),
  hideLimitsPopover: vi.fn(() => Promise.resolve()),
  openLimitsWindow: vi.fn(() => Promise.resolve()),
  quitApp: vi.fn(() => Promise.resolve()),
  onSharedReadChanged: vi.fn(() => Promise.resolve(() => undefined)),
}));

it("selects the lightweight Limits surface from the window query", () => {
  render(<Root search="?surface=limits-popover" />);

  expect(screen.getByRole("heading", { name: "Limits" })).toBeTruthy();
});
