import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
import { formatObservedAt } from "$lib/limitsFormat";
import { refreshLimits } from "./useLimitsProviders";
import { Limits } from "./Limits";

const readAccounts = vi.hoisted(() => vi.fn());
const addAccount = vi.hoisted(() => vi.fn());
const accountAction = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const forgetLimitsSnapshot = vi.hoisted(() => vi.fn());

const sharedReadHandlers = vi.hoisted(() => new Set<(change: { source: string }) => void>());
const onSharedReadChanged = vi.hoisted(() => (handler: (change: { source: string }) => void) => {
  sharedReadHandlers.add(handler);
  return Promise.resolve(() => { sharedReadHandlers.delete(handler); });
});
const consumeCodexResetCredit = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({ readLimits, readAccounts, accountAction, readAccountPreferences:vi.fn().mockResolvedValue(false), readAccountActivationBlockers:vi.fn().mockResolvedValue([]), addAccount, cancelAccountLogin:vi.fn(), forgetLimitsSnapshot, onSharedReadChanged, readCodexSubscription: vi.fn().mockResolvedValue(null), consumeCodexResetCredit }));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const NOW = "2026-08-17T20:00:00Z";

function okClaude(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "claude",
    status: "ok",
    account: { id: "uuid-1", label: "me@claude.example" },
    currentAccount: true,
    plan: "max",
    windows: [
      { id: "weekly_all", label: "Weekly · all models", kind: "weekly", usedPercent: 12, resetsAt: "2026-08-24T13:59:59Z", observedAt: NOW },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 7, resetsAt: "2026-08-18T04:59:59Z", observedAt: NOW },
      { id: "weekly_opus", label: "Weekly · Opus", kind: "model", usedPercent: 91.4, resetsAt: "2026-08-24T13:59:59Z", observedAt: NOW },
    ],
    ...overrides,
  };
}

function okCodex(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "codex",
    status: "ok",
    account: { id: "acct-work", label: "work@codex.example" },
    currentAccount: true,
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 74, resetsAt: "2026-08-24T23:34:33Z", observedAt: NOW },
      { id: "extra:gpt-5.6-luna", label: "Weekly · GPT-5.6-Luna", kind: "model", usedPercent: 3, resetsAt: "2026-08-17T19:59:00Z", observedAt: NOW },
    ],
    credits: { balance: "12.5", unlimited: false },
    ...overrides,
  };
}

/** A remembered snapshot of the other Codex account: read yesterday, one window already reset. */
function staleCodex(): ProviderLimits {
  return {
    provider: "codex",
    status: "ok",
    account: { id: "acct-personal", label: "personal@codex.example" },
    currentAccount: false,
    plan: "plus",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 88, resetsAt: "2026-08-20T10:00:00Z", observedAt: "2026-08-16T21:40:00.000Z" },
      { id: "secondary", label: "5 hour · all models", kind: "session", usedPercent: 40, resetsAt: "2026-08-16T22:00:00Z", observedAt: "2026-08-16T21:40:00.000Z" },
    ],
  };
}

function statusOnly(provider: AgentId, status: LimitsStatus, message: string | null): ProviderLimits {
  return { provider, status, message, currentAccount: true, windows: [] };
}

function answer(
  claude: ProviderLimits[] | Promise<ProviderLimits[]>,
  codex: ProviderLimits[] | Promise<ProviderLimits[]>,
) {
  readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? claude : codex));
}

function renderLimits(pollMinutes: LimitsPollMinutes = 5) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <Limits pollMinutes={pollMinutes} />
    </QueryClientProvider>,
  );
  return { ...view, client };
}

function card(name: string) {
  return screen.getByRole("region", { name });
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readAccounts.mockReset().mockResolvedValue({profiles:[],nativeAccount:null,recoveryRequired:false,notice:null});
  addAccount.mockReset().mockResolvedValue(undefined);
  accountAction.mockReset().mockResolvedValue(undefined);
  readLimits.mockReset();
  consumeCodexResetCredit.mockReset();
  forgetLimitsSnapshot.mockReset();
  forgetLimitsSnapshot.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Limits", () => {
  it("draws the active account's dot beside its headline window, or in its header without one", async () => {
    const session = { id: "secondary", label: "5 hour · all models", kind: "session" as const, usedPercent: 12, resetsAt: "2026-08-17T23:00:00Z", observedAt: NOW };
    answer([okClaude({ plan: "max ×5" })], [okCodex({ windows: [session] }), staleCodex()]);
    renderLimits();
    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const beside = within(claude).getByRole("img", { name: "Active account" });
    expect(within(beside.parentElement!).getByText("Weekly · all models")).toBeTruthy();
    expect(claude.querySelector("header")!.contains(beside)).toBe(false);
    expect(within(claude).queryByText("ACTIVE")).toBeNull();
    expect(within(claude).getByText("Max ×5")).toBeTruthy();
    const codex = card("Codex limits · work@codex.example");
    const dots = within(codex).getAllByRole("img", { name: "Active account" });
    expect(dots).toHaveLength(1);
    expect(codex.querySelector("header")!.contains(dots[0])).toBe(true);
    expect(within(codex).getByRole("meter", { name: "5 hour · all models" })).toBeTruthy();
    expect(within(card("Codex limits · personal@codex.example")).queryByRole("img", { name: "Active account" })).toBeNull();
  });

  it("opens account actions from the header and dismisses them with Escape or an outside click", async () => {
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    const trigger = await screen.findByRole("button", {name: "More actions for personal@codex.example"});
    expect(trigger.closest("header")).not.toBeNull();
    const remembered = card("Codex limits · personal@codex.example");
    expect(remembered.getAttribute("data-current-account")).toBe("false");
    expect(within(remembered).getByRole("button", { name: "Sign in" })).toBeTruthy();
    fireEvent.click(trigger);
    expect(screen.getByRole("group", {name: "Actions for personal@codex.example"})).toBeTruthy();
    expect(screen.getByRole("button", {name: "Remove account"})).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, {key: "Escape"});
    expect(screen.queryByRole("group", {name: "Actions for personal@codex.example"})).toBeNull();
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.pointerDown(screen.getByRole("heading", {name: "Subscription limits"}));
    expect(screen.queryByRole("group", {name: "Actions for personal@codex.example"})).toBeNull();
    expect(accountAction).not.toHaveBeenCalled();
  });

  it("asks for both subscriptions (not forced) and renders visible windows with percent, fill and reset", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits();

    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro ×20")).toBeTruthy());
    expect(readLimits).toHaveBeenCalledWith("claude", false);
    expect(readLimits).toHaveBeenCalledWith("codex", false);
    expect(readLimits).toHaveBeenCalledTimes(2);

    const claude = card("Claude limits · me@claude.example");
    expect(claude.getAttribute("data-status")).toBe("ok");
    expect(claude.getAttribute("data-current-account")).toBe("true");
    const weekly = within(claude).getByRole("meter", { name: "Weekly · all models" });
    expect(weekly.getAttribute("aria-valuenow")).toBe("12");
    expect((weekly.firstElementChild as HTMLElement).style.backgroundColor).toBe("rgb(217, 119, 87)");
    expect(within(claude).getByText("12%")).toBeTruthy();
    expect(within(claude).getAllByText(/resets in 6d 17h/)).toHaveLength(2);
    expect(within(claude).getByText(/resets in 8h 59m/)).toBeTruthy();
    const opus = within(claude).getByRole("meter", { name: "Weekly · Opus" });
    expect((opus.firstElementChild as HTMLElement).style.backgroundColor).toBe("var(--trip)");
    expect(within(claude).getByText("91%")).toBeTruthy();
    expect(within(claude).getByRole("meter", { name: "5 hour · all models" }).getAttribute("aria-valuenow")).toBe("7");

    const codex = card("Codex limits · work@codex.example");
    expect(
      (within(codex).getByRole("meter", { name: "Weekly · all models" }).firstElementChild as HTMLElement).style
        .backgroundColor,
    ).toBe("color-mix(in srgb, var(--silkscreen), var(--trip) 44.7%)");
    // Credits read as a row under the windows, not a chip crowding the header.
    const header = codex.querySelector("header")!;
    expect(within(header).queryByText(/credits/i)).toBeNull();
    expect(header.textContent).not.toContain("12.5");
    expect(within(codex).getByRole("definition", { name: "Credits" }).textContent).toBe("12.5");
    // A current-account observation is still historical after its own reset instant passes.
    const luna = within(codex).getByRole("meter", { name: "Weekly · GPT-5.6-Luna" });
    expect(luna.getAttribute("aria-valuenow")).toBe("0");
    // The renewed window reads as the zero it is, in ordinary ink.
    expect(within(codex).getByText("0%").style.color).toBe("");
    expect(within(codex).queryByText("—")).toBeNull();
    // The reset is a fact worth stating: when it happened. What it held before is not.
    expect(within(codex).getByText(/^reset 1m ago · \w{3} \d\d:\d\d$/)).toBeTruthy();
    expect(within(codex).queryByText(/last seen/)).toBeNull();
    expect(within(codex).queryByText(/Current usage unknown/)).toBeNull();
    // Every meter simply speaks its percentage; there is no reset voice-over.
    expect(luna.getAttribute("aria-valuetext")).toBeNull();
    expect(weekly.getAttribute("aria-valuetext")).toBeNull();
    expect(within(codex).getAllByText(/Latest observation/)).toHaveLength(1);
    expect(within(claude).getAllByText(/Latest observation/)).toHaveLength(1);
    expect(within(codex).getAllByText(/resets in/)).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /^Forget/ })).toBeNull();
    expect(screen.getByRole("heading", { name: "Subscription limits" })).toBeTruthy();
    // The caption carries the poll interval the user configured, the way Pull requests does.
    expect(screen.getByText("every 5 minutes")).toBeTruthy();

  });

  // The fill used to step to `--warn` at 70 %, which is lighter than the accent it replaced. It now
  // blends toward `--trip`, so a meter inside the band carries a mix and never the amber.
  it("hardens a high-usage fill toward red instead of stepping to amber", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits();

    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const opus = within(claude).getByRole("meter", { name: "Weekly · Opus" });
    const codex = screen.getByRole("region", { name: "Codex limits · work@codex.example" });
    const weekly = within(codex).getByRole("meter", { name: "Weekly · all models" });

    expect((opus.firstElementChild as HTMLElement).style.backgroundColor).toBe("var(--trip)");
    const fill = (weekly.firstElementChild as HTMLElement).style.backgroundColor;
    expect(fill).toContain("var(--trip)");
    expect(fill).not.toContain("var(--warn)");
  });

  it("says the signed-in account's refresh is paused above its message, with nothing to sign into or forget", async () => {
    answer([okClaude({ status: "unauthenticated", message: "Access token expired — send a prompt with `claude` to renew it, then refresh here." })], [okCodex()]);
    renderLimits();
    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    expect(within(claude).getByText("Refresh paused.")).toBeTruthy();
    expect(within(claude).getByText(/^Access token expired/)).toBeTruthy();
    expect(within(claude).getByRole("meter", { name: "Weekly · all models" }).getAttribute("aria-valuenow")).toBe("12");
    expect(within(claude).queryByText(/Claude Desktop usage/)).toBeNull();
    expect(within(claude).queryByText(/sign in/)).toBeNull();
    expect(within(claude).queryByRole("button", { name: /^Forget/ })).toBeNull();
  });

  it("re-reads every provider on focus only after the configured interval", async () => {
    answer([statusOnly("claude", "unauthenticated", "Access token expired")], [okCodex()]);
    renderLimits();
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(2));

    await act(async () => {
      window.dispatchEvent(new Event("visibilitychange"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(readLimits).toHaveBeenCalledTimes(2);

    vi.setSystemTime(new Date("2026-08-17T20:05:01Z"));
    await act(async () => {
      window.dispatchEvent(new Event("visibilitychange"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(4));
    expect(readLimits.mock.calls.slice(2)).toEqual([
      ["claude", false],
      ["codex", false],
    ]);
  });

  it("uses a longer configured interval without an early focus refresh", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits(10);
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(2));

    vi.setSystemTime(new Date("2026-08-17T20:05:01Z"));
    await act(async () => window.dispatchEvent(new Event("visibilitychange")));
    expect(readLimits).toHaveBeenCalledTimes(2);

    vi.setSystemTime(new Date("2026-08-17T20:10:01Z"));
    await act(async () => window.dispatchEvent(new Event("visibilitychange")));
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(4));
  });

  it("forgets a remembered account on request and drops its card without a refetch", async () => {
    const pending = deferred<void>();
    forgetLimitsSnapshot.mockReturnValue(pending.promise);
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());

    fireEvent.click(screen.getByLabelText("More actions for personal@codex.example"));
    const remove = screen.getByRole("button", { name: "Remove account" });
    await waitFor(() => expect(remove).toBeEnabled());
    fireEvent.click(remove);
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(forgetLimitsSnapshot).toHaveBeenCalledWith("codex", "acct-personal"));
    // The card only goes once the backend has actually forgotten it.
    expect(card("Codex limits · personal@codex.example")).toBeTruthy();
    await act(async () => {
      pending.resolve();
    });
    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    expect(card("Codex limits · work@codex.example")).toBeTruthy();
    expect(readLimits).toHaveBeenCalledTimes(2);
  });

  it("keeps remembered accounts visible when the current login is signed out", async () => {
    answer([okClaude()], [statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits."), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());
    const current = card("Codex limits");
    expect(within(current).getByText("Sign in with `codex` to see subscription limits.")).toBeTruthy();
    expect(current.getAttribute("data-status")).toBe("signedOut");
  });

  it("keeps the card and reports the error when Forget fails", async () => {
    forgetLimitsSnapshot.mockRejectedValue(new Error("locked"));
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("More actions for personal@codex.example"));
    const remove = screen.getByRole("button", { name: "Remove account" });
    await waitFor(() => expect(remove).toBeEnabled());
    fireEvent.click(remove);
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(screen.getByText(/Could not remove that account: locked/)).toBeTruthy());
    expect(card("Codex limits · personal@codex.example")).toBeTruthy();
  });

  it.each([true, false])("puts Claude's subscription status beside the plan and says when it is only the last known (%s)", async (lastKnown) => {
    answer([okClaude({ subscriptionStatus: "past_due", ...(lastKnown ? { status: "unauthenticated" as const, message: "Sign in again." } : {}) })], []);
    renderLimits();
    const claude = await waitFor(() => card("Claude limits · me@claude.example"));
    const badge = within(claude).getByRole("button", { name: "Subscription status: Payment due" });
    expect(badge.closest("header")).toHaveTextContent("Max");
    fireEvent.focus(badge);
    const tooltip = screen.getByRole("tooltip");
    expect(tooltip).toHaveTextContent(`Checked ${formatObservedAt(NOW)}`);
    if (lastKnown) expect(tooltip).toHaveTextContent("Last known subscription status.");
    else expect(tooltip).not.toHaveTextContent("Last known");
  });

  it("shows banked resets and a paid offer as rows, and offers the Codex reset beside the account actions", async () => {
    const banked = { availableCount: 2, nextExpiresAt: null };
    answer([okClaude({ resetCredits: banked })], [okCodex({ resetCredits: banked, resetOffer: { price: { currency: "USD", amountMinorUnits: 800 } } })]);
    renderLimits();

    const current = await waitFor(() => card("Codex limits · work@codex.example"));
    const button = await within(current).findByRole("button", { name: "Use banked reset" });
    expect(button.closest("footer")).toBe(within(current).getByRole("button", { name: "Save account" }).closest("footer"));
    // Like Save account, it waits until the native login is confirmed to be this card's account.
    expect(button).toHaveProperty("disabled", true);
    expect(within(current).getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(within(current).getByRole("definition", { name: "Paid reset" })).toHaveTextContent("$8.00");
    // on-n-off spends only Codex resets; the signed-in Claude card says where Claude Code spends its own.
    const claude = card("Claude limits · me@claude.example");
    expect(within(claude).getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(within(claude).getByText("/limit-reset in Claude Code")).toBeTruthy();
    expect(within(claude).queryByRole("button", { name: "Use banked reset" })).toBeNull();
  });

  it("shows the spent reset on the refreshed card the backend announces", async () => {
    const spent = okCodex({ windows: okCodex().windows.map((window) => ({ ...window, usedPercent: 99 })), resetCredits: { availableCount: 1, nextExpiresAt: null } });
    answer([okClaude()], [spent]);
    consumeCodexResetCredit.mockImplementation(async () => {
      // The backend replaces the shared Codex read before it answers, then announces it.
      answer([okClaude()], [okCodex({ windows: okCodex().windows.map((window) => ({ ...window, usedPercent: 0 })), resetCredits: { availableCount: 0, nextExpiresAt: null } })]);
      for (const handler of sharedReadHandlers) handler({ source: "limits:codex" });
      return "reset";
    });
    // The native login is the card's account, so the account controls leave the button usable.
    readAccounts.mockResolvedValue({ profiles: [], nativeObservationId: "acct-work", nativeAccount: null, recoveryRequired: false, notice: null });
    renderLimits();

    const current = await waitFor(() => card("Codex limits · work@codex.example"));
    const button = await within(current).findByRole("button", { name: "Use banked reset" });
    await waitFor(() => expect(button).toHaveProperty("disabled", false));
    fireEvent.click(button);

    await waitFor(() => expect(within(current).queryByRole("definition", { name: "Banked resets" })).toBeNull());
    expect(within(current).getByRole("status").textContent).toBe("Banked reset used.");
    expect(within(current).queryByRole("button", { name: "Use banked reset" })).toBeNull();
  });

  it("shows a checking state until the first answer arrives", async () => {
    const pending = deferred<ProviderLimits[]>();
    answer(pending.promise, [okCodex()]);
    renderLimits();
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro ×20")).toBeTruthy());
    const claude = card("Claude limits");
    expect(claude.getAttribute("data-status")).toBe("pending");
    expect(within(claude).getByText(/Checking limits/)).toBeTruthy();
    expect(screen.getByTestId("limits-screen")).toHaveAttribute("aria-busy", "true");
    await act(async () => {
      pending.resolve([okClaude()]);
    });
    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    expect(screen.queryByText(/Checking limits/)).toBeNull();

  });

  it("refreshes both providers with force and keeps the last good numbers when a refresh fails", async () => {
    let claudeCalls = 0;
    readLimits.mockImplementation((agentId: AgentId) => {
      if (agentId === "codex") return Promise.resolve([okCodex()]);
      claudeCalls += 1;
      return claudeCalls === 1 ? Promise.resolve([okClaude()]) : Promise.reject(new Error("worker failed"));
    });
    const view = renderLimits();
    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro ×20")).toBeTruthy());

    await act(async () => { await refreshLimits(view.client); });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(4));
    expect(readLimits).toHaveBeenLastCalledWith(expect.any(String), true);
    expect(readLimits.mock.calls.slice(2).every((call) => call[1] === true)).toBe(true);
    await waitFor(() => expect(screen.getByText(/worker failed/)).toBeTruthy());
    const claude = card("Claude limits · me@claude.example");
    expect(claude.getAttribute("data-status")).toBe("ok");
    expect(within(claude).getByRole("meter", { name: "Weekly · all models" }).getAttribute("aria-valuenow")).toBe("12");

    // Force is one-shot: a background refetch after the button goes back to a plain read.
    await act(async () => {
      await view.client.invalidateQueries({ queryKey: ["limits"] });
    });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(6));
    expect(readLimits.mock.calls.slice(4).every((call) => call[1] === false)).toBe(true);
  });
});

import { readCodexSubscription } from "$lib/api";

it("reads each Codex card's subscription date and offers no billing action", async () => {
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValue(null);
  answer([okClaude()], [okCodex({ currentAccount: false })]);
  renderLimits();
  const region = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
  await waitFor(() => expect(readCodexSubscription).toHaveBeenCalledWith("acct-work"));
  fireEvent.click(within(region).getByRole("button", { name: "More actions for work@codex.example" }));
  const menu = within(region).getByRole("group", { name: "Actions for work@codex.example" });
  expect(within(menu).queryByRole("button", { name: /billing/i })).toBeNull();
  expect(accountAction).not.toHaveBeenCalled();
  fireEvent.keyDown(menu, { key: "Escape" });
  expect(within(region).getByRole("button", { name: "More actions for work@codex.example" })).toHaveFocus();
  expect(readCodexSubscription).not.toHaveBeenCalledWith("uuid-1");
});

it.each([true, false])("shows the paid-through date through a header badge (usage available: %s)", async (hasUsage) => {
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValueOnce({ date: "2026-10-10T12:00:00Z", checkedAt: NOW });
  const codex = okCodex();
  if (!hasUsage) codex.windows = [];
  answer([okClaude()], [codex]);
  renderLimits();
  const badge = await screen.findByRole("button", {name: "Subscription paid through Oct 10"});
  expect(badge.closest("header")).toHaveTextContent("Pro ×20");
  fireEvent.focus(badge);
  expect(screen.getByRole("tooltip")).toHaveTextContent(/Paid through/);
  fireEvent.keyDown(document, {key:"Escape"});
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(readCodexSubscription).toHaveBeenCalledTimes(1);
});

it("places switching on the account usage card without a second account manager", async()=>{
 answer([okClaude()], [okCodex(),staleCodex()]);
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?[{id:"saved-personal",observationId:"acct-personal",identity:{provider:"codex",userId:"personal",workspaceId:"personal"},email:"personal@codex.example",label:"personal@codex.example",category:"Personal",savedAt:NOW,active:false,needsLogin:false}]:[],nativeAccount:null,recoveryRequired:false,notice:null}));
 renderLimits();
 const personal=await screen.findByRole("region",{name:"Codex limits · personal@codex.example"});
 const use=await within(personal).findByRole("button",{name:"Use account"});
 expect(within(personal).getByRole("meter",{name:"Weekly · all models"})).toBeVisible();
 expect(within(personal).getByText("Personal")).toBeVisible();
 expect(screen.queryByRole("button",{name:/Manage .* accounts/})).toBeNull();
 fireEvent.click(use);
 await waitFor(()=>expect(accountAction).toHaveBeenCalledWith("codex","use","saved-personal",undefined));
});

it("keeps saved accounts available when usage cannot be read, never merges by email, and never shows workspace ids", async () => {
 answer([okClaude()], Promise.reject(new Error("usage offline")));
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?["ws-personal-7f3a","ws-business-9c1e"].map(id=>({id,observationId:`profile:${id}`,identity:{provider:"codex",userId:"same-user",workspaceId:id},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:false,needsLogin:false})):[],nativeAccount:null,recoveryRequired:false,notice:null}));
 renderLimits();
 const cards=await screen.findAllByRole("region",{name:"Codex limits · shared@example.com"});
 expect(cards).toHaveLength(2);
 for (const card of cards) expect(card.textContent).not.toMatch(/ws-personal-7f3a|ws-business-9c1e/);
 expect(within(cards[0]).getByRole("button",{name:"Use account"})).toBeEnabled();
 expect(within(cards[1]).getByRole("button",{name:"Use account"})).toBeEnabled();
});

it("does not let an old current card save or sign out a different native account", async () => {
 answer([okClaude()], [okCodex()]);
 readAccounts.mockResolvedValue({profiles:[],nativeObservationId:"different-native-account",nativeAccount:{provider:"codex",userId:"b",workspaceId:"b"},recoveryRequired:false,notice:null});
 renderLimits();
 const current=await screen.findByRole("region",{name:"Codex limits · work@codex.example"});
 expect(within(current).getByRole("button",{name:"Save account"})).toBeDisabled();
 fireEvent.click(within(current).getByLabelText("More actions for work@codex.example"));
 expect(within(current).getByRole("button",{name:"Sign out"})).toBeDisabled();
 expect(accountAction).not.toHaveBeenCalled();
});

it("keeps old account controls disabled until the post-switch account read finishes", async () => {
 answer([okClaude()], [okCodex(), staleCodex()]);
 const next = deferred<unknown>();
 const accounts={profiles:[{id:"saved-personal",observationId:"acct-personal",identity:{provider:"codex",userId:"personal",workspaceId:"personal"},email:"personal@codex.example",label:"personal@codex.example",savedAt:NOW,active:false,needsLogin:false}],nativeObservationId:"acct-work",nativeAccount:{provider:"codex",userId:"work",workspaceId:"work"},recoveryRequired:false,notice:null};
 let codexReads=0;
 readAccounts.mockImplementation(async(provider:string)=>provider==="codex"?(++codexReads===1?accounts:next.promise):{profiles:[],nativeAccount:null,recoveryRequired:false,notice:null});
 renderLimits();
 const old=await screen.findByRole("region",{name:"Codex limits · work@codex.example"});
 const target=await screen.findByRole("region",{name:"Codex limits · personal@codex.example"});
 fireEvent.click(await within(target).findByRole("button",{name:"Use account"}));
 await waitFor(()=>expect(codexReads).toBe(2));
 expect(within(old).getByRole("button",{name:"Save account"})).toBeDisabled();
 await act(async()=>next.resolve({...accounts,nativeObservationId:"acct-personal"}));
});

it("refuses an already-open sign-out confirmation after the native identity changes", async () => {
 answer([okClaude()], [okCodex()]);
 const accounts={profiles:[],nativeObservationId:"acct-work",nativeAccount:{provider:"codex",userId:"work",workspaceId:"work"},recoveryRequired:false,notice:null};
 readAccounts.mockResolvedValue(accounts);
 const {client}=renderLimits();
 const current=await screen.findByRole("region",{name:"Codex limits · work@codex.example"});
 fireEvent.click(within(current).getByLabelText("More actions for work@codex.example"));
 const signOut=within(current).getByRole("button",{name:"Sign out"});
 await waitFor(()=>expect(signOut).toBeEnabled());
 fireEvent.click(signOut);
 await act(async()=>{client.setQueryData(["accounts","codex"],{...accounts,nativeObservationId:"different"});});
 const confirm=within(current).getByRole("button",{name:"Confirm sign out"});
 await waitFor(()=>expect(confirm).toBeDisabled()); fireEvent.click(confirm);
 expect(accountAction).not.toHaveBeenCalled();
});

it.each(["claude", "codex"])("adds only the %s account selected in the shared picker", async provider => {
  answer([okClaude()], [okCodex()]);
  renderLimits();
  const add = screen.getByRole("button", {name: "Add account"});
  fireEvent.click(add);
  expect(addAccount).not.toHaveBeenCalled();
  const picker = screen.getByRole("group", {name: "Add account"});
  const preference = within(picker).getByRole("checkbox", {name: "Automatically save accounts I sign in to"});
  await waitFor(() => expect(preference).toBeEnabled());
  expect(preference).not.toBeChecked();
  fireEvent.click(preference);
  fireEvent.blur(preference, { relatedTarget: null });
  expect(screen.getByRole("group", {name: "Add account"})).toBe(picker);
  await waitFor(() => expect(accountAction).toHaveBeenCalledWith("codex", "remember"));
  fireEvent.click(within(picker).getByRole("button", {name: provider === "claude" ? "Claude" : "Codex"}));
  await waitFor(() => expect(addAccount).toHaveBeenCalledWith(provider, expect.any(String), undefined));
  expect(addAccount).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("group", {name: "Add account"})).toBeNull();
});

it("queues an explicit reload behind an in-flight background check", async () => {
  const pending = deferred<ProviderLimits[]>();
  readLimits.mockImplementation((provider: AgentId, force: boolean) => provider === "claude" && !force ? pending.promise : Promise.resolve(provider === "claude" ? [okClaude({windows: []})] : [okCodex()]));
  const view = renderLimits();
  await waitFor(() => expect(readLimits).toHaveBeenCalledWith("claude", false));
  await act(async () => {
    const refreshed = refreshLimits(view.client);
    pending.resolve([okClaude()]);
    await refreshed;
  });
  expect(readLimits).toHaveBeenCalledWith("claude", true);
  expect(view.client.getQueryData<ProviderLimits[]>(["limits", "claude"])?.[0].windows).toEqual([]);
});

it("removes reconciled legacy history with the saved card so refresh cannot resurrect it", async () => {
 const profile = {id:"saved",observationId:"profile:user-team",identity:{provider:"codex",userId:"user",workspaceId:"team"},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:false,needsLogin:false};
 let profiles = [profile];
 let history = [okCodex({account:{id:profile.observationId,label:profile.email},currentAccount:false}),okCodex({account:{id:"team",label:profile.email},currentAccount:false}),staleCodex()];
 readLimits.mockImplementation(async(provider:string)=>provider==="codex"?history:[okClaude()]);
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?profiles:[],nativeAccount:null,recoveryRequired:false,notice:null}));
 accountAction.mockImplementation(async()=>{profiles=[];});
 forgetLimitsSnapshot.mockImplementation(async(_provider:string,id:string)=>{history=history.filter(entry=>entry.account?.id!==id);});
 const {client}=renderLimits();
 await screen.findByRole("button",{name:"Use account"});
 fireEvent.click(screen.getByRole("button",{name:"More actions for shared@example.com"}));
 fireEvent.click(screen.getByRole("button",{name:"Remove account"}));
 fireEvent.click(screen.getByRole("button",{name:"Confirm removal"}));
 await waitFor(()=>expect(accountAction).toHaveBeenCalled());
 await waitFor(()=>expect(forgetLimitsSnapshot).toHaveBeenCalledWith("codex","profile:user-team"));
 // Legacy history goes first, so a partial failure keeps the scoped observation available.
 expect(forgetLimitsSnapshot.mock.calls).toEqual([["codex","team","shared@example.com"],["codex","profile:user-team"]]);
 await act(async()=>{await client.invalidateQueries({queryKey:["limits","codex"]});});
 expect(screen.queryByRole("region",{name:"Codex limits · shared@example.com"})).toBeNull();
 expect(card("Codex limits · personal@codex.example")).toBeTruthy();
});


it.each([true, false])("shows the billing term through a header badge (usage available: %s)", async (hasUsage) => {
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValueOnce(null);
  const codex = okCodex({ subscription: { activeUntil: "2026-10-10T12:00:00Z", willRenew: false, note: "cancelled", checkedAt: NOW } });
  if (!hasUsage) codex.windows = [];
  answer([okClaude()], [codex]);
  renderLimits();
  const badge = await screen.findByRole("button", {name: "Subscription status: No renewal"});
  expect(badge.closest("header")).toHaveTextContent("Pro ×20");
  fireEvent.focus(badge);
  expect(screen.getByRole("tooltip")).toHaveTextContent(/Expires/);
  expect(screen.getByRole("tooltip")).toHaveTextContent("Cancelled: the plan ends with this period.");
  fireEvent.keyDown(document, {key:"Escape"});
  expect(screen.queryByRole("tooltip")).toBeNull();
});

it("turns a plan's last minute red on the local minute clock without rereading", async () => {
  vi.useFakeTimers({toFake:["Date", "setInterval", "clearInterval"]});
  vi.setSystemTime(new Date(NOW));
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValueOnce(null);
  answer([okClaude()], [okCodex({ subscription: { activeUntil: new Date(Date.parse(NOW) + 30_000).toISOString(), willRenew: false, checkedAt: NOW } })]);
  renderLimits();
  const badge = await screen.findByRole("button", {name: "Subscription status: No renewal"});
  expect(badge).toHaveClass("subscription-badge--warning");
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(badge).toHaveClass("subscription-badge--expired");
  expect(readCodexSubscription).toHaveBeenCalledTimes(1);
});

it("drops the badge on the local minute clock once the paid period ends, without rereading", async () => {
  vi.useFakeTimers({toFake:["Date", "setInterval", "clearInterval"]});
  vi.setSystemTime(new Date(NOW));
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValueOnce({
    date: new Date(Date.parse(NOW) + 30_000).toISOString(), checkedAt: NOW,
  });
  answer([okClaude()], [okCodex()]);
  renderLimits();
  expect(await screen.findByRole("button", {name: /Subscription paid through/})).toHaveClass("subscription-badge--neutral");
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(screen.queryByRole("button", {name: /Subscription paid through/})).toBeNull();
  expect(readCodexSubscription).toHaveBeenCalledTimes(1);
});

it("quietly identifies last-known usage and reveals the reason on focus", async () => {
  const reason = "Saved usage credential is no longer accepted.";
  answer([okClaude()], [okCodex({currentAccount:false, status:"failed", message:reason})]);
  renderLimits();
  const region = await screen.findByRole("region", {name: "Codex limits · work@codex.example"});
  const badge = within(region).getByRole("button", {name:"Usage status: Last known usage"});
  expect(within(region).queryByText(reason)).toBeNull();
  expect(within(region).getAllByRole("meter").length).toBeGreaterThan(0);
  expect(within(region).getByText(/Latest observation/)).toBeInTheDocument();
  fireEvent.focus(badge);
  expect(screen.getByRole("tooltip")).toHaveTextContent(reason);
  fireEvent.keyDown(document, {key:"Escape"});
  expect(screen.queryByRole("tooltip")).toBeNull();
});

it("shows the copy the card model gives an empty card", async () => {
  const saved = (id: string) => ({ id, observationId: `profile:${id}`, identity: { provider: "codex", userId: id, workspaceId: id }, email: `${id}@codex.example`, label: `${id}@codex.example`, savedAt: NOW, active: false, needsLogin: false });
  answer([okClaude({ windows: [], account: null })], [okCodex(), okCodex({ account: { id: "profile:empty", label: "empty@codex.example" }, currentAccount: false, windows: [], credits: null })]);
  readAccounts.mockImplementation(async (provider: string) => ({ profiles: provider === "codex" ? [saved("unread"), saved("empty")] : [], nativeAccount: null, recoveryRequired: false, notice: null }));
  renderLimits();
  expect(within(await screen.findByRole("region", { name: "Codex limits · unread@codex.example" })).getByText("Usage unavailable.")).toBeTruthy();
  expect(within(card("Codex limits · empty@codex.example")).getByText("Usage unavailable.")).toBeTruthy();
  expect(within(card("Claude limits")).getByText("Claude reported no rate-limit windows.")).toBeTruthy();
});
