import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { LimitsPopover } from "./LimitsPopover";

const readLimits = vi.hoisted(() => vi.fn());
const onLimitsPopoverOpened = vi.hoisted(() => vi.fn());
const hideLimitsPopover = vi.hoisted(() => vi.fn());
const openLimitsWindow = vi.hoisted(() => vi.fn());
const quitApp = vi.hoisted(() => vi.fn());
const loadAppSettings = vi.hoisted(() => vi.fn());
let openedHandler: (() => void) | null = null;

vi.mock("$lib/api", () => ({
  readLimits,
  onLimitsPopoverOpened,
  hideLimitsPopover,
  openLimitsWindow,
  quitApp,
  loadAppSettings,
  onSharedReadChanged: () => Promise.resolve(() => undefined),
}));

function limits(
  provider: AgentId,
  id: string,
  label: string,
  currentAccount: boolean,
): ProviderLimits {
  return {
    provider,
    status: "ok",
    account: { id, label },
    currentAccount,
    plan: provider === "claude" ? "max" : "pro",
    windows: [
      {
        id: "weekly_all",
        label: "Weekly · all models",
        kind: "weekly",
        usedPercent: currentAccount ? 24 : 52,
        resetsAt: "2026-08-25T12:00:00Z",
        observedAt: currentAccount ? "2026-08-18T12:00:00Z" : "2026-08-17T12:00:00Z",
      },
    ],
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
            limits("claude", "claude-current", "current@claude.example", true),
            limits("claude", "claude-kept", "kept@claude.example", false),
          ]
        : [
            limits("codex", "codex-current", "current@codex.example", true),
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
  loadAppSettings.mockReset();
  loadAppSettings.mockResolvedValue({ limitsPollMinutes: 5 });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("LimitsPopover", () => {
  it("shows a window that reset since its observation as unobserved, not as its old percentage", async () => {
    const claude = limits("claude", "claude-current", "current@claude.example", true);
    claude.windows = [
      { id: "weekly_all", label: "Weekly · all models", kind: "weekly", usedPercent: 24, resetsAt: "2026-08-25T12:00:00Z", observedAt: "2026-08-18T12:00:00Z" },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 93, resetsAt: "2026-08-18T12:58:00Z", observedAt: "2026-08-18T12:00:00Z" },
    ];
    readLimits.mockImplementation((provider: AgentId) => Promise.resolve(provider === "claude" ? [claude] : []));
    renderPopover();

    const account = await screen.findByRole("article", { name: "Claude limits · current@claude.example" });
    const session = within(account).getByRole("meter", { name: "5 hour · all models" });
    expect(session.getAttribute("aria-valuenow")).toBe("0");
    expect(session.getAttribute("aria-valuetext")).toBeNull();
    expect((session.firstElementChild as HTMLElement).style.backgroundColor).not.toBe("var(--trip)");
    expect(within(account).getByText("0%").style.color).toBe("");
    expect(within(account).getByText(/^reset 2m ago · \w{3} \d\d:\d\d$/)).toBeTruthy();
    // The live window keeps its number.
    const weekly = within(account).getByRole("meter", { name: "Weekly · all models" });
    expect(weekly.getAttribute("aria-valuenow")).toBe("24");
    expect(weekly.getAttribute("aria-valuetext")).toBeNull();
    expect(within(account).getByText("24%").style.color).toBe("");
  });


  it("groups current and remembered accounts under compact provider sections", async () => {
    renderPopover();

    const claude = await screen.findByRole("region", { name: "Claude accounts" });
    const codex = screen.getByRole("region", { name: "Codex accounts" });
    expect((await within(claude).findAllByRole("article")).map((entry) => entry.getAttribute("aria-label"))).toEqual([
      "Claude limits · current@claude.example",
      "Claude limits · kept@claude.example",
    ]);
    expect(within(codex).getAllByRole("article").map((entry) => entry.getAttribute("aria-label"))).toEqual([
      "Codex limits · current@codex.example",
      "Codex limits · kept@codex.example",
    ]);
    expect(screen.getAllByText("Remembered account")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /^Forget/ })).toBeNull();
  });

  it("uses the critical tone for a nearly exhausted meter fill", async () => {
    const claude = limits("claude", "claude-current", "current@claude.example", true);
    claude.windows[0].usedPercent = 90;
    readLimits.mockImplementation((provider: AgentId) =>
      Promise.resolve(provider === "claude" ? [claude] : []),
    );
    renderPopover();

    const meter = await screen.findByRole("meter", { name: "Weekly · all models" });

    expect((meter.firstElementChild as HTMLElement).style.backgroundColor).toBe("var(--trip)");
  });

  it("keeps the current account's observations when refresh is paused", async () => {
    readLimits.mockImplementation((provider: AgentId) =>
      Promise.resolve(
        provider === "claude"
          ? [
              {
                ...limits("claude", "claude-current", "current@claude.example", true),
                status: "unauthenticated" as const,
                message: "Access token expired — send a prompt with `claude` to renew it, then refresh here.",
              },
            ]
          : [limits("codex", "codex-current", "current@codex.example", true)],
      ),
    );
    renderPopover();

    const claude = await screen.findByRole("region", { name: "Claude accounts" });
    const account = await within(claude).findByRole("article", { name: "Claude limits · current@claude.example" });
    expect(within(account).getByText(/^Access token expired/)).toBeTruthy();
    expect(within(account).getByRole("meter", { name: "Weekly · all models" }).getAttribute("aria-valuenow")).toBe("24");
    expect(within(account).getByText("Refresh paused")).toBeTruthy();
    expect(within(account).getAllByText(/Latest observation/)).toHaveLength(1);
  });

  it("forces both provider reads when Refresh is selected", async () => {
    renderPopover();
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(4));

    fireEvent.click(screen.getByRole("button", { name: "Refresh limits" }));

    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(4));
    expect(readLimits.mock.calls.slice(2)).toEqual([
      ["claude", true],
      ["codex", true],
    ]);
    expect(screen.getAllByRole("article")).toHaveLength(4);
  });

  it("keeps cached accounts visible and shows a provider refresh error", async () => {
    renderPopover();
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(4));
    readLimits.mockRejectedValueOnce({ kind: "message", message: "Claude refresh failed" });

    fireEvent.click(screen.getByRole("button", { name: "Refresh limits" }));

    expect(await screen.findByRole("alert", { name: "Claude refresh error" })).toHaveTextContent(
      "Claude refresh failed",
    );
    expect(screen.getAllByRole("article")).toHaveLength(4);
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

    vi.setSystemTime(new Date("2026-08-18T13:05:01Z"));
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

it("marks saved usage quietly in the popover and exposes its failure on focus", async () => {
  const reason = "Saved usage credential is no longer accepted.";
  readLimits.mockImplementation((provider: AgentId) => Promise.resolve(provider === "claude"
    ? [{...limits("claude", "saved", "you@example.com", false), status:"unauthenticated", message:reason}] : []));
  renderPopover();
  const account = await screen.findByRole("article", {name:"Claude limits · you@example.com"});
  const badge = within(account).getByRole("button", {name:"Usage status: Last known usage"});
  expect(within(account).queryByText(reason)).toBeNull();
  // One status per card: the last-known badge, not the "Remembered account" pill as well.
  expect(within(account).queryByText("Remembered account")).toBeNull();
  expect(within(account).getByRole("meter")).toBeInTheDocument();
  expect(within(account).getByText(/Latest observation/)).toBeInTheDocument();
  fireEvent.focus(badge);
  expect(screen.getByRole("tooltip")).toHaveTextContent(reason);
});

it("says an answered read with no windows has none, as the Limits screen words it", async () => {
  readLimits.mockImplementation((provider: AgentId) => Promise.resolve(provider === "claude"
    ? [{ ...limits("claude", "claude-current", "current@claude.example", true), windows: [] }] : [limits("codex", "codex-current", "current@codex.example", true)]));
  renderPopover();
  const account = await screen.findByRole("article", { name: "Claude limits · current@claude.example" });
  expect(within(account).getByText("Claude reported no rate-limit windows.")).toBeInTheDocument();
  expect(within(account).queryByText("No rate-limit windows.")).toBeNull();
});

it.each([
  ["by its account's label", { id: "claude-1", label: "you@example.com" }, "Claude limits · you@example.com", "you@example.com"],
  ["by its provider when its account has no label", { id: "claude-1", label: null }, "Claude limits", "Claude"],
  ["by its provider when it has no account", null, "Claude limits", "Claude"],
] as const)("names a card %s, as the Limits screen does", async (_case, account, name, label) => {
  readLimits.mockImplementation((provider: AgentId) => Promise.resolve(provider === "claude"
    ? [{ provider: "claude", status: "signedOut", message: "Sign in with `claude`.", account, currentAccount: true, windows: [] }] : [limits("codex", "codex-current", "current@codex.example", true)]));
  renderPopover();
  const article = await screen.findByRole("article", { name });
  expect(within(article).getByText(label)).toBeInTheDocument();
  expect(within(article).getByText("Sign in with `claude`.")).toBeInTheDocument();
});

it("says a provider with no accounts has none saved", async () => {
  readLimits.mockImplementation((provider: AgentId) => Promise.resolve(provider === "claude" ? [] : [limits("codex", "codex-current", "current@codex.example", true)]));
  renderPopover();
  const claude = await screen.findByRole("region", { name: "Claude accounts" });
  await waitFor(() => expect(within(claude).getByText("No saved accounts.")).toBeInTheDocument());
});
