import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { LimitsPopover } from "./LimitsPopover";

const readLimits = vi.hoisted(() => vi.fn());
const onLimitsPopoverOpened = vi.hoisted(() => vi.fn());
const hideLimitsPopover = vi.hoisted(() => vi.fn());
const openLimitsWindow = vi.hoisted(() => vi.fn());
const quitApp = vi.hoisted(() => vi.fn());
let openedHandler: (() => void) | null = null;

vi.mock("$lib/api", () => ({
  readLimits,
  onLimitsPopoverOpened,
  hideLimitsPopover,
  openLimitsWindow,
  quitApp,
}));

function limits(
  provider: AgentId,
  id: string,
  label: string,
  live: boolean,
): ProviderLimits {
  return {
    provider,
    status: "ok",
    account: { id, label },
    live,
    plan: provider === "claude" ? "max" : "pro",
    windows: [
      {
        id: "weekly_all",
        label: "Weekly · all models",
        kind: "weekly",
        usedPercent: live ? 24 : 52,
        resetsAt: "2026-08-25T12:00:00Z",
      },
    ],
    fetchedAt: live ? "2026-08-18T12:00:00Z" : "2026-08-17T12:00:00Z",
  };
}

function renderPopover() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  return render(
    <QueryClientProvider client={client}>
      <LimitsPopover />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-08-18T13:00:00Z"));
  readLimits.mockReset();
  readLimits.mockImplementation((provider: AgentId) =>
    Promise.resolve(
      provider === "claude"
        ? [
            limits("claude", "claude-live", "live@claude.example", true),
            limits("claude", "claude-kept", "kept@claude.example", false),
          ]
        : [
            limits("codex", "codex-live", "live@codex.example", true),
            limits("codex", "codex-kept", "kept@codex.example", false),
          ],
    ),
  );
  onLimitsPopoverOpened.mockReset();
  openedHandler = null;
  onLimitsPopoverOpened.mockImplementation((handler: () => void) => {
    openedHandler = handler;
    return Promise.resolve(() => undefined);
  });
  hideLimitsPopover.mockReset();
  hideLimitsPopover.mockResolvedValue(undefined);
  openLimitsWindow.mockReset();
  openLimitsWindow.mockResolvedValue(undefined);
  quitApp.mockReset();
  quitApp.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("LimitsPopover", () => {
  it("shows every live and remembered account grouped in provider order", async () => {
    renderPopover();

    await waitFor(() => expect(screen.getAllByRole("region")).toHaveLength(4));
    expect(screen.getAllByRole("region").map((region) => region.getAttribute("aria-label"))).toEqual([
      "Claude limits · live@claude.example",
      "Claude limits · kept@claude.example",
      "Codex limits · live@codex.example",
      "Codex limits · kept@codex.example",
    ]);
    expect(screen.queryByRole("button", { name: /^Forget/ })).toBeNull();
  });

  it("forces both provider reads when Refresh is selected", async () => {
    renderPopover();
    await waitFor(() => expect(screen.getAllByRole("region")).toHaveLength(4));

    fireEvent.click(screen.getByRole("button", { name: "Refresh limits" }));

    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(4));
    expect(readLimits.mock.calls.slice(2)).toEqual([
      ["claude", true],
      ["codex", true],
    ]);
    expect(screen.getAllByRole("region")).toHaveLength(4);
  });

  it("hides the popover when Escape is pressed", async () => {
    renderPopover();

    fireEvent.keyDown(document, { key: "Escape" });

    await waitFor(() => expect(hideLimitsPopover).toHaveBeenCalledTimes(1));
  });

  it("opens the full Limits screen from the footer", async () => {
    renderPopover();

    fireEvent.click(screen.getByRole("button", { name: "Open on-n-off" }));

    await waitFor(() => expect(openLimitsWindow).toHaveBeenCalledTimes(1));
  });

  it("shows a local error when the main window cannot be opened", async () => {
    openLimitsWindow.mockRejectedValueOnce({ kind: "message", message: "main window unavailable" });
    renderPopover();

    fireEvent.click(screen.getByRole("button", { name: "Open on-n-off" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not open on-n-off: main window unavailable",
    );
  });

  it("quits the application from the footer", async () => {
    renderPopover();

    fireEvent.click(screen.getByRole("button", { name: "Quit" }));

    await waitFor(() => expect(quitApp).toHaveBeenCalledTimes(1));
  });

  it("refetches stale provider data when the retained popover reopens", async () => {
    renderPopover();
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(openedHandler).not.toBeNull());

    vi.setSystemTime(new Date("2026-08-18T13:00:30Z"));
    await act(async () => openedHandler?.());
    expect(readLimits).toHaveBeenCalledTimes(2);

    vi.setSystemTime(new Date("2026-08-18T13:01:01Z"));
    await act(async () => openedHandler?.());
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(4));
    expect(readLimits.mock.calls.slice(2)).toEqual([
      ["claude", false],
      ["codex", false],
    ]);
  });

  it("reapplies the stored theme each time the popover opens", async () => {
    document.documentElement.classList.remove("dark");
    localStorage.setItem("on-n-off.theme", "dark");
    renderPopover();

    await waitFor(() => expect(document.documentElement.classList.contains("dark")).toBe(true));
    await waitFor(() => expect(openedHandler).not.toBeNull());

    localStorage.setItem("on-n-off.theme", "light");
    await act(async () => openedHandler?.());
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(document.documentElement.style.colorScheme).toBe("light");
  });
});
