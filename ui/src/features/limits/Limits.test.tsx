import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LimitsColumnEntry } from "$lib/limitsTypes";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
import { SharedReads } from "$lib/sharedReads/announcements";
import { refreshLimits } from "$lib/sharedReads/limits";
import { Limits } from "./Limits";
import { NOW, REMEMBERED, lastObserved, okClaude, okCodex, staleCodex, statusOnly, withSavedProfiles } from "./readingFixtures";

const readAccounts = vi.hoisted(() => vi.fn());
const addAccount = vi.hoisted(() => vi.fn());
const accountAction = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const forgetLimitsSnapshot = vi.hoisted(() => vi.fn());
const setLimitsArchived = vi.hoisted(() => vi.fn());

const sharedReadHandlers = vi.hoisted(() => new Set<(change: { source: string }) => void>());
const onSharedReadChanged = vi.hoisted(() => (handler: (change: { source: string }) => void) => {
  sharedReadHandlers.add(handler);
  return Promise.resolve(() => { sharedReadHandlers.delete(handler); });
});
const consumeCodexResetCredit = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({ readLimits, readAccounts, accountAction, readAccountPreferences:vi.fn().mockResolvedValue(false), readAccountActivationBlockers:vi.fn().mockResolvedValue([]), addAccount, cancelAccountLogin:vi.fn(), forgetLimitsSnapshot, setLimitsArchived, setAccountCategory: vi.fn(), onSharedReadChanged, readCodexSubscription: vi.fn().mockResolvedValue(null), consumeCodexResetCredit, pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn() }));

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

function answer(
  claude: LimitsColumnEntry[] | Promise<LimitsColumnEntry[]>,
  codex: LimitsColumnEntry[] | Promise<LimitsColumnEntry[]>,
) {
  readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? claude : agentId === "codex" ? codex : []));
}

function renderLimits(pollMinutes: LimitsPollMinutes = 5) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <SharedReads />
      <Limits pollMinutes={pollMinutes} resetAlerts={{}} onResetAlertsChange={async () => undefined} />
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
  setLimitsArchived.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Limits", () => {
  it("renders the Muse column from its own read, with no account controls", async () => {
    readLimits.mockImplementation((agentId: AgentId) =>
      Promise.resolve(
        agentId === "muse"
          ? [{
              provider: "muse" as const,
              status: "ok" as const,
              account: { id: "email:you@example.com", label: "you@example.com" },
              currentAccount: true,
              plan: "wonder",
              planLabel: "Wonder",
              windows: [
                { id: "weekly", label: "Weekly · all models", kind: "weekly" as const, usedPercent: 22, resetsAt: "2026-08-24T20:00:00Z", observedAt: NOW },
                { id: "session", label: "5 hour · all models", kind: "session" as const, usedPercent: 41, resetsAt: "2026-08-17T22:15:00Z", windowSeconds: 18_000, observedAt: NOW },
              ],
            }]
          : agentId === "claude"
            ? [okClaude()]
            : [okCodex()],
      ));
    renderLimits();
    const muse = await screen.findByRole("region", { name: "Muse limits · you@example.com" });
    expect(within(muse).getByText("Wonder")).toBeTruthy();
    expect(within(muse).getByRole("meter", { name: "Weekly · all models" }).getAttribute("aria-valuenow")).toBe("22");
    expect(within(muse).getByRole("meter", { name: "5 hour · all models" }).getAttribute("aria-valuenow")).toBe("41");
    expect(within(muse).queryByRole("button", { name: /^Sign in/ })).toBeNull();
  });

  it("says under a row why it shows no reset", async () => {
    const remembered = okClaude({ currentAccount: false, standing: REMEMBERED, account: { id: "uuid-2", label: "other@claude.example" } });
    remembered.windows[1] = { ...remembered.windows[1], usedPercent: 0, resetsAt: null, startsOnFirstMessage: true };
    const codex = okCodex();
    codex.windows[1] = { ...codex.windows[1], resetsAt: null };
    answer([okClaude(), remembered], [codex]);
    renderLimits();
    const row = (region: string, meter: string) => within(card(region)).getByRole("meter", { name: meter });
    await screen.findByRole("region", { name: "Claude limits · other@claude.example" });
    expect(row("Claude limits · other@claude.example", "5 hour · all models")).toHaveAccessibleDescription("Starts with your first message");
    expect(row("Codex limits · work@codex.example", "Weekly · GPT-5.6-Luna")).toHaveAccessibleDescription("Reset time unavailable");
  });

  it("opens account actions from the header and dismisses them with Escape or an outside click", async () => {
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    const trigger = await screen.findByRole("button", {name: "More actions for personal@codex.example"});
    expect(trigger.closest("header")).not.toBeNull();
    const remembered = card("Codex limits · personal@codex.example");
    expect(within(remembered).getByRole("button", { name: "Sign in" })).toBeTruthy();
    fireEvent.click(trigger);
    expect(screen.getByRole("group", {name: "Actions for personal@codex.example"})).toBeTruthy();
    expect(screen.getByRole("button", {name: "Check limits now"})).toHaveFocus();
    expect(screen.getByRole("button", {name: "Archive account"})).toBeInTheDocument();
    expect(screen.getByRole("button", {name: "Remove account"})).toBeInTheDocument();
    fireEvent.keyDown(document.activeElement!, {key: "Escape"});
    expect(screen.queryByRole("group", {name: "Actions for personal@codex.example"})).toBeNull();
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.pointerDown(screen.getByRole("heading", {name: "Subscription limits"}));
    expect(screen.queryByRole("group", {name: "Actions for personal@codex.example"})).toBeNull();
    expect(accountAction).not.toHaveBeenCalled();
  });

  it("asks for every subscription (not forced) and renders visible windows with percent, fill and reset", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits();

    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro ×20")).toBeTruthy());
    expect(readLimits).toHaveBeenCalledWith("claude", false);
    expect(readLimits).toHaveBeenCalledWith("codex", false);
    expect(readLimits).toHaveBeenCalledWith("muse", false);
    expect(readLimits).toHaveBeenCalledTimes(3);

    const claude = card("Claude limits · me@claude.example");
    expect(claude.getAttribute("data-status")).toBe("ok");
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
    const header = codex.querySelector("header")!;
    expect(within(header).queryByText(/credits/i)).toBeNull();
    expect(header.textContent).not.toContain("12.5");
    expect(within(codex).getByRole("definition", { name: "Credits" }).textContent).toBe("12.5");
    const luna = within(codex).getByRole("meter", { name: "Weekly · GPT-5.6-Luna" });
    expect(luna.getAttribute("aria-valuenow")).toBe("0");
    expect(within(codex).getByText("0%").style.color).toBe("");
    expect(within(codex).queryByText("—")).toBeNull();
    expect(within(codex).getByText(/^reset 1m ago · \w{3} \d\d:\d\d$/)).toBeTruthy();
    expect(within(codex).queryByText(/last seen/)).toBeNull();
    expect(within(codex).queryByText(/Current usage unknown/)).toBeNull();
    expect(luna.getAttribute("aria-valuetext")).toBeNull();
    expect(weekly.getAttribute("aria-valuetext")).toBeNull();
    expect(within(codex).getAllByText(/Latest observation/)).toHaveLength(1);
    expect(within(claude).getAllByText(/Latest observation/)).toHaveLength(1);
    expect(within(codex).getAllByText(/resets in/)).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /^Forget/ })).toBeNull();
    expect(screen.getByRole("heading", { name: "Subscription limits" })).toBeTruthy();
    expect(screen.getByText("every 5 minutes")).toBeTruthy();

  });

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

  it("turns a spent headline's figure red, as its meter is", async () => {
    const spent = okClaude();
    spent.windows = [{ ...spent.windows[0], usedPercent: 95 }];
    answer([spent], [okCodex()]);
    renderLimits();

    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    expect(within(claude).getByText("95%")).toHaveStyle({ color: "var(--trip)" });
  });

  it("says the signed-in account's refresh is paused above its message, with nothing to sign into or forget", async () => {
    const message = "Access token expired — send a prompt with `claude` to renew it, then refresh here.";
    answer([okClaude({ status: "unauthenticated", standing: lastObserved("readPaused", message, "muted") })], [okCodex()]);
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
    answer([statusOnly("claude", "unauthenticated", "Access token expired", "muted")], [okCodex()]);
    renderLimits();
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(3));

    await act(async () => {
      window.dispatchEvent(new Event("visibilitychange"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(readLimits).toHaveBeenCalledTimes(3);

    vi.setSystemTime(new Date("2026-08-17T20:05:01Z"));
    await act(async () => {
      window.dispatchEvent(new Event("visibilitychange"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(6));
    expect(readLimits.mock.calls.slice(3)).toEqual([
      ["claude", false],
      ["codex", false],
      ["muse", false],
    ]);
  });

  it("checks the shared cache on focus after five minutes with a longer configured interval", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits(10);
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(3));

    vi.setSystemTime(new Date("2026-08-17T20:05:01Z"));
    await act(async () => window.dispatchEvent(new Event("visibilitychange")));
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(6));
    expect(readLimits.mock.calls.slice(3)).toEqual([["claude", false], ["codex", false], ["muse", false]]);

    vi.setSystemTime(new Date("2026-08-17T20:10:02Z"));
    await act(async () => window.dispatchEvent(new Event("visibilitychange")));
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(9));
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
    await waitFor(() => expect(forgetLimitsSnapshot).toHaveBeenCalledWith("codex", "acct-personal", undefined));
    expect(card("Codex limits · personal@codex.example")).toBeTruthy();
    await act(async () => {
      pending.resolve();
    });
    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    expect(card("Codex limits · work@codex.example")).toBeTruthy();
    expect(readLimits).toHaveBeenCalledTimes(3);
  });

  it("keeps remembered accounts visible when the current login is signed out", async () => {
    answer([okClaude()], [statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.", "muted"), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());
    const current = card("Codex limits");
    expect(within(current).getByText("Sign in with `codex` to see subscription limits.")).toBeTruthy();
    expect(current.getAttribute("data-status")).toBe("signedOut");
    expect(within(current).queryByRole("button", { name: /Sign in/ })).toBeNull();
    fireEvent.click(within(current).getByRole("button", { name: "More actions for Codex" }));
    expect(within(screen.getByRole("group", { name: "Actions for Codex" })).getAllByRole("button").map(item => item.textContent)).toEqual(["Check limits now"]);
  });

  it("draws a failed read's message as an error and a prompt to sign in as muted text", async () => {
    answer([statusOnly("claude", "failed", "Could not reach the Claude usage service (HTTP 503).", "error")], [statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.", "muted")]);
    renderLimits();
    const failed = await waitFor(() => within(card("Claude limits")).getByText("Could not reach the Claude usage service (HTTP 503)."));
    expect(failed).toHaveClass("text-[var(--trip)]");
    const prompt = await waitFor(() => within(card("Codex limits")).getByText("Sign in with `codex` to see subscription limits."));
    expect(prompt).toHaveClass("text-[var(--mute)]");
    expect(prompt).not.toHaveClass("text-[var(--trip)]");
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

  it("shows banked resets and a paid offer as rows, and offers the Codex reset beside the account actions", async () => {
    const banked = { availableCount: 2, nextExpiresAt: null };
    answer([okClaude()], [okCodex({ resetCredits: banked, resetOffer: { price: { currency: "USD", amountMinorUnits: 800 } } })]);
    renderLimits();

    const current = await waitFor(() => card("Codex limits · work@codex.example"));
    const button = await within(current).findByRole("button", { name: "Use banked reset" });
    expect(button.closest("footer")).toBe(within(current).getByRole("button", { name: "Save account" }).closest("footer"));
    expect(button).toHaveProperty("disabled", true);
    expect(within(current).getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(within(current).getByRole("definition", { name: "Paid reset" })).toHaveTextContent("$8.00");
    const claude = card("Claude limits · me@claude.example");
    expect(within(claude).queryByRole("button", { name: "Use banked reset" })).toBeNull();
  });

  it("shows the spent reset on the refreshed card the backend announces", async () => {
    const spent = okCodex({ windows: okCodex().windows.map((window) => ({ ...window, usedPercent: 99 })), resetCredits: { availableCount: 1, nextExpiresAt: null } });
    answer([okClaude()], [spent]);
    consumeCodexResetCredit.mockImplementation(async () => {
      answer([okClaude()], [okCodex({ windows: okCodex().windows.map((window) => ({ ...window, usedPercent: 0 })), resetCredits: { availableCount: 0, nextExpiresAt: null } })]);
      for (const handler of sharedReadHandlers) handler({ source: "limits:codex" });
      return "reset";
    });
    readAccounts.mockResolvedValue({ profiles: [], nativeObservationId: "acct-work", nativeAccount: null, recoveryRequired: false, notice: null });
    renderLimits();

    const current = await waitFor(() => card("Codex limits · work@codex.example"));
    const button = await within(current).findByRole("button", { name: "Use banked reset" });
    await waitFor(() => expect(button).toHaveProperty("disabled", false));
    fireEvent.click(button);
    fireEvent.click(within(screen.getByRole("alertdialog", { name: "Use this reset?" })).getByRole("button", { name: "Use reset" }));

    await waitFor(() => expect(within(current).queryByRole("definition", { name: "Banked resets" })).toBeNull());
    expect(within(current).getByRole("status").textContent).toBe("Banked reset used.");
    expect(within(current).queryByRole("button", { name: "Use banked reset" })).toBeNull();
  });

  it("shows a checking state until the first answer arrives", async () => {
    const pending = deferred<LimitsColumnEntry[]>();
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

  it("refreshes every provider with force and keeps the last good numbers when a refresh fails", async () => {
    let claudeCalls = 0;
    readLimits.mockImplementation((agentId: AgentId) => {
      if (agentId === "muse") return Promise.resolve([]);
      if (agentId === "codex") return Promise.resolve([okCodex()]);
      claudeCalls += 1;
      return claudeCalls === 1 ? Promise.resolve([okClaude()]) : Promise.reject(new Error("worker failed"));
    });
    const view = renderLimits();
    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro ×20")).toBeTruthy());

    await act(async () => { await refreshLimits(view.client); });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(6));
    expect(readLimits).toHaveBeenLastCalledWith(expect.any(String), true);
    expect(readLimits.mock.calls.slice(3).every((call) => call[1] === true)).toBe(true);
    await waitFor(() => expect(screen.getByText(/worker failed/)).toBeTruthy());
    const claude = card("Claude limits · me@claude.example");
    expect(claude.getAttribute("data-status")).toBe("ok");
    expect(within(claude).getByRole("meter", { name: "Weekly · all models" }).getAttribute("aria-valuenow")).toBe("12");

    await act(async () => {
      await view.client.invalidateQueries({ queryKey: ["limits"] });
    });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(9));
    expect(readLimits.mock.calls.slice(6).every((call) => call[1] === false)).toBe(true);
  });
});

import { readCodexSubscription } from "$lib/api";

it("reads each Codex card's subscription date and offers no billing action", async () => {
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValue(null);
  answer([okClaude()], [okCodex({ currentAccount: false, standing: REMEMBERED })]);
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

it("shows a category edited elsewhere once the Limits read it comes with is announced", async () => {
  answer([okClaude()], [okCodex()]);
  renderLimits();
  const codex = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
  expect(within(codex).queryByText("Renamed elsewhere")).toBeNull();
  answer([okClaude()], [okCodex({ category: "Renamed elsewhere" })]);

  await act(async () => { for (const handler of sharedReadHandlers) handler({ source: "limits:codex" }); });

  expect(await within(codex).findByText("Renamed elsewhere")).toBeInTheDocument();
});

it("places switching on the account usage card without a second account manager", async()=>{
 const saved={id:"saved-personal",observationId:"acct-personal",legacyId:"personal",identity:{provider:"codex" as const,userId:"personal",workspaceId:"personal"},email:"personal@codex.example",label:"personal@codex.example",category:"Personal",savedAt:NOW,active:false,loginState:{state:"usable" as const}};
 answer([okClaude()], withSavedProfiles([okCodex(),staleCodex({category:"Personal"})],[saved]));
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?[saved]:[],nativeAccount:null,recoveryRequired:false,notice:null}));
 renderLimits();
 const personal=await screen.findByRole("region",{name:"Codex limits · personal@codex.example"});
 const use=await within(personal).findByRole("button",{name:"Use account"});
 expect(within(personal).getByRole("meter",{name:"Weekly · all models"})).toBeVisible();
 expect(within(personal).getByText("Personal")).toBeVisible();
 expect(screen.queryByRole("button",{name:/Manage .* accounts/})).toBeNull();
 fireEvent.click(use);
 await waitFor(()=>expect(accountAction).toHaveBeenCalledWith("codex","use","saved-personal"));
});

it("keeps saved accounts usable when no read answered for them, never merges by email, and never shows workspace ids", async () => {
 const profiles=["ws-personal-7f3a","ws-business-9c1e"].map(id=>({id,observationId:`profile:${id}`,legacyId:id,identity:{provider:"codex" as const,userId:"same-user",workspaceId:id},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:false,loginState:{state:"usable" as const}}));
 answer([okClaude()], withSavedProfiles([], profiles));
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?profiles:[],nativeAccount:null,recoveryRequired:false,notice:null}));
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
 const next = deferred<unknown>();
 const accounts={profiles:[{id:"saved-personal",observationId:"acct-personal",legacyId:"personal",identity:{provider:"codex" as const,userId:"personal",workspaceId:"personal"},email:"personal@codex.example",label:"personal@codex.example",savedAt:NOW,active:false,loginState:{state:"usable" as const}}],nativeObservationId:"acct-work",nativeAccount:{provider:"codex",userId:"work",workspaceId:"work"},recoveryRequired:false,notice:null};
 answer([okClaude()], withSavedProfiles([okCodex(), staleCodex()], accounts.profiles));
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

it("lays out one column per provider with limits, in tab order, and offers to add an account only where accounts can be saved", async () => {
  answer([okClaude()], [okCodex()]);
  renderLimits();
  await screen.findByRole("region", { name: /^Codex limits/ });
  const columns = screen.getByTestId("limits-screen").querySelector(".grid")!.children;
  expect([...columns].map((column) => within(column as HTMLElement).getAllByRole("region")[0]?.getAttribute("aria-label")?.split(" ")[0])).toEqual(["Claude", "Codex", "Muse"]);

  fireEvent.click(screen.getByRole("button", {name: "Add account"}));
  const picker = screen.getByRole("group", {name: "Add account"});
  expect(within(picker).getAllByRole("button", { name: /^(Claude|Codex|Antigravity|Cursor|Muse)$/ }).map((button) => button.lastChild?.textContent)).toEqual(["Claude", "Codex", "Muse"]);
});

it.each(["claude", "codex", "muse"])("adds only the %s account selected in the shared picker", async provider => {
  answer([okClaude()], [okCodex()]);
  renderLimits();
  const add = screen.getByRole("button", {name: "Add account"});
  fireEvent.click(add);
  expect(addAccount).not.toHaveBeenCalled();
  const picker = screen.getByRole("group", {name: "Add account"});
  const preference = within(picker).getByRole("button", {name: "Automatically save accounts I sign in to"});
  await waitFor(() => expect(preference).toBeEnabled());
  expect(preference).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(preference);
  fireEvent.blur(preference, { relatedTarget: null });
  expect(screen.getByRole("group", {name: "Add account"})).toBe(picker);
  await waitFor(() => expect(accountAction).toHaveBeenCalledWith("codex", "remember"));
  fireEvent.click(within(picker).getByRole("button", {name: {claude: "Claude", codex: "Codex", muse: "Muse"}[provider]}));
  await waitFor(() => expect(addAccount).toHaveBeenCalledWith(provider, expect.any(String), undefined));
  expect(addAccount).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("group", {name: "Add account"})).toBeNull();
});

it("queues an explicit reload behind an in-flight background check", async () => {
  const pending = deferred<LimitsColumnEntry[]>();
  readLimits.mockImplementation((provider: AgentId, force: boolean) => provider === "claude" && !force ? pending.promise : Promise.resolve(provider === "claude" ? [okClaude({windows: []})] : provider === "codex" ? [okCodex()] : []));
  const view = renderLimits();
  await waitFor(() => expect(readLimits).toHaveBeenCalledWith("claude", false));
  await act(async () => {
    const refreshed = refreshLimits(view.client);
    pending.resolve([okClaude()]);
    await refreshed;
  });
  expect(readLimits).toHaveBeenCalledWith("claude", true);
  expect(view.client.getQueryData<LimitsColumnEntry[]>(["limits", "claude"])?.[0].windows).toEqual([]);
});

it("removes reconciled legacy history with the saved card so refresh cannot resurrect it", async () => {
 const profile = {id:"saved",observationId:"profile:user-team",legacyId:"team",identity:{provider:"codex" as const,userId:"user",workspaceId:"team"},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:false,loginState:{state:"usable" as const}};
 let profiles = [profile];
 let history = [okCodex({account:{id:profile.observationId,label:profile.email},currentAccount:false,standing:REMEMBERED,profile,replaced:[{id:"team",label:profile.email}]}),staleCodex()];
 readLimits.mockImplementation(async(provider:string)=>provider==="codex"?history:provider==="claude"?[okClaude()]:[]);
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?profiles:[],nativeAccount:null,recoveryRequired:false,notice:null}));
 accountAction.mockImplementation(async()=>{profiles=[];});
 forgetLimitsSnapshot.mockImplementation(async(_provider:string,id:string)=>{history=history.filter(entry=>entry.account?.id!==id);});
 const {client}=renderLimits();
 await screen.findByRole("button",{name:"Use account"});
 fireEvent.click(screen.getByRole("button",{name:"More actions for shared@example.com"}));
 fireEvent.click(screen.getByRole("button",{name:"Remove account"}));
 expect(accountAction).not.toHaveBeenCalled();
 fireEvent.click(screen.getByRole("button",{name:"Confirm removal"}));
 await waitFor(()=>expect(accountAction).toHaveBeenCalledWith("codex","remove","saved"));
 expect(accountAction.mock.calls.map(call=>call[1])).not.toContain("signOut");
 await waitFor(()=>expect(forgetLimitsSnapshot).toHaveBeenCalledWith("codex","profile:user-team",undefined));
 expect(forgetLimitsSnapshot.mock.calls).toEqual([["codex","team","shared@example.com"],["codex","profile:user-team",undefined]]);
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
  expect(badge).toHaveClass("term-badge--warning");
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(badge).toHaveClass("term-badge--expired");
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
  expect(await screen.findByRole("button", {name: /Subscription paid through/})).toHaveClass("term-badge--neutral");
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(screen.queryByRole("button", {name: /Subscription paid through/})).toBeNull();
  expect(readCodexSubscription).toHaveBeenCalledTimes(1);
});

it("says a remembered reading is a remembered account, and no saved profile's reading is one", async () => {
  answer([okClaude()], [
    okCodex(),
    staleCodex(),
    okCodex({ account: { id: "acct-live", label: "live@codex.example" }, currentAccount: false, savedProfile: true }),
    okCodex({ account: { id: "acct-saved", label: "saved@codex.example" }, currentAccount: false, savedProfile: true, status: "failed",
      standing: lastObserved("savedRefreshPaused", "Saved usage credential is no longer accepted.", "error") }),
  ]);
  renderLimits();
  const remembered = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
  const status = within(remembered).getByText("Remembered account.");
  expect(remembered.querySelector("header")!.contains(status)).toBe(false);
  expect(within(card("Codex limits · work@codex.example")).queryByText(/Remembered account/)).toBeNull();
  expect(within(card("Codex limits · live@codex.example")).queryByText(/Remembered account/)).toBeNull();
  const failed = card("Codex limits · saved@codex.example");
  expect(within(failed).getByRole("button", { name: "Usage status: Last known usage" })).toBeTruthy();
  expect(within(failed).queryByText(/Remembered account/)).toBeNull();
});

it("quietly identifies last-known usage and reveals the reason on focus", async () => {
  const reason = "Saved usage credential is no longer accepted.";
  answer([okClaude()], [okCodex({currentAccount:false, savedProfile:true, status:"failed", standing:lastObserved("savedRefreshPaused", reason, "error")})]);
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
  const saved = (id: string) => ({ id, observationId: `profile:${id}`, legacyId: id, identity: { provider: "codex" as const, userId: id, workspaceId: id }, email: `${id}@codex.example`, label: `${id}@codex.example`, savedAt: NOW, active: false, loginState: { state: "usable" as const } });
  answer([okClaude({ windows: [], account: null })], withSavedProfiles(
    [okCodex(), okCodex({ account: { id: "profile:empty", label: "empty@codex.example" }, currentAccount: false, windows: [], credits: null })],
    [saved("unread"), saved("empty")],
  ));
  renderLimits();
  expect(within(await screen.findByRole("region", { name: "Codex limits · unread@codex.example" })).getByText("Usage unavailable.")).toBeTruthy();
  expect(within(card("Codex limits · empty@codex.example")).getByText("Usage unavailable.")).toBeTruthy();
  expect(within(card("Claude limits")).getByText("Claude reported no rate-limit windows.")).toBeTruthy();
});

describe("archiving", () => {
  it("archives a card from its menu without asking and lists it under the column's archived accounts", async () => {
    const spare = staleCodex({ account: { id: "acct-spare", label: "spare@codex.example" } });
    const signedOut = statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.", "muted");
    answer([okClaude()], [signedOut, staleCodex(), spare]);
    renderLimits();
    await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    fireEvent.click(screen.getByRole("button", { name: "More actions for me@claude.example" }));
    expect(screen.queryByRole("button", { name: "Archive account" }), "never the signed-in card").toBeNull();
    fireEvent.keyDown(screen.getByRole("button", { name: "More actions for me@claude.example" }), { key: "Escape" });

    fireEvent.click(screen.getByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));

    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith("codex", ["acct-personal"], true));
    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    const disclosure = screen.getByRole("button", { name: "Archived (1)" });
    expect(disclosure, "focus follows the card instead of dropping to the page").toHaveFocus();
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(card("Codex limits · spare@codex.example"), "only the card archived moves").toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull();
  });

  it("leaves focus where the user moved it while the archive waited", async () => {
    const archiving = deferred<void>();
    setLimitsArchived.mockReturnValue(archiving.promise);
    answer([okClaude()], [okCodex(), staleCodex(), staleCodex({ account: { id: "acct-spare", label: "spare@codex.example" } })]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));
    const elsewhere = screen.getByRole("button", { name: "More actions for spare@codex.example" });
    elsewhere.focus();

    await act(async () => { archiving.resolve(); });

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    expect(screen.getByRole("button", { name: "Archived (1)" })).toBeInTheDocument();
    expect(elsewhere).toHaveFocus();
  });

  it("archives and unarchives a saved login no read answered for, through the Limits entry it comes with", async () => {
    const unread = { id: "unread", observationId: "profile:unread", legacyId: "team", identity: { provider: "codex" as const, userId: "unread", workspaceId: "team" }, email: "unread@codex.example", label: "unread@codex.example", savedAt: NOW, active: false, loginState: { state: "usable" as const } };
    answer([okClaude()], withSavedProfiles([okCodex()], [unread]));
    readAccounts.mockImplementation(async (provider: AgentId) => ({ profiles: provider === "codex" ? [unread] : [], nativeAccount: null, recoveryRequired: false, notice: null }));
    const { client } = renderLimits();
    const archivedFlag = () => client.getQueryData<LimitsColumnEntry[]>(["limits", "codex"])?.find(entry => entry.unread)?.archived;
    fireEvent.click(await screen.findByRole("button", { name: "More actions for unread@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));

    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith("codex", ["profile:unread"], true));
    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · unread@codex.example" })).toBeNull());
    expect(archivedFlag()).toBe(true);
    expect(screen.getByRole("button", { name: "Archived (1)" }), "its own entry moved it, not a read").toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Archived (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "Unarchive unread@codex.example" }));

    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith("codex", ["profile:unread"], false));
    expect(await screen.findByRole("region", { name: "Codex limits · unread@codex.example" })).toBeInTheDocument();
    expect(archivedFlag()).toBe(false);
  });

  it("removes an archived history card by forgetting each of its snapshots, with no saved login to remove", async () => {
    answer([okClaude()], [okCodex(), staleCodex({ archived: true })]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove account personal@codex.example" })).toBeEnabled());

    fireEvent.click(screen.getByRole("button", { name: "Remove account personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    await waitFor(() => expect(forgetLimitsSnapshot.mock.calls).toEqual([["codex", "acct-personal", undefined]]));
    expect(accountAction).not.toHaveBeenCalled();
  });

  it("keeps the card and says why when archiving it fails", async () => {
    setLimitsArchived.mockRejectedValue({ kind: "message", message: "disk full", path: null });
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));

    expect(await screen.findByText("Could not archive that account: disk full")).toBeInTheDocument();
    expect(card("Codex limits · personal@codex.example")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Archived/ })).toBeNull();
  });

  it("unarchives an account in place, and leaves reading it again to the backend, which announces its forced read", async () => {
    let archived = true;
    readLimits.mockImplementation(async (provider: AgentId) => provider === "codex"
      ? [okCodex(), staleCodex({ archived })] : provider === "claude" ? [okClaude()] : []);
    setLimitsArchived.mockImplementation(async () => { archived = false; });
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    const reads = readLimits.mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: "Unarchive personal@codex.example" }));

    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith("codex", ["acct-personal"], false));
    expect(await screen.findByRole("region", { name: "Codex limits · personal@codex.example" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Archived/ })).toBeNull();
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    expect(readLimits.mock.calls.slice(reads), "no read of the UI's own").toEqual([]);

    await act(async () => { for (const handler of sharedReadHandlers) handler({ source: "limits:codex" }); });
    await waitFor(() => expect(readLimits.mock.calls.slice(reads)).toEqual([["codex", false]]));
  });

  it("unarchives an account in the Muse column, which has no account manager, and hands focus to the next archived row", async () => {
    const weekly = { id: "weekly", label: "Weekly · all models", kind: "weekly" as const, usedPercent: 30, resetsAt: "2026-08-24T20:00:00Z", observedAt: NOW };
    const archived = new Set(["muse-old", "muse-spare"]);
    const muse = (id: string, label: string, currentAccount = false): LimitsColumnEntry =>
      ({ provider: "muse", status: "ok", account: { id, label }, currentAccount, plan: "wonder", planLabel: "Wonder", windows: [weekly], archived: archived.has(id) });
    readLimits.mockImplementation(async (provider: AgentId) => provider === "muse"
      ? [muse("muse-now", "now@muse.example", true), muse("muse-old", "old@muse.example"), muse("muse-spare", "spare@muse.example")]
      : provider === "claude" ? [okClaude()] : [okCodex()]);
    setLimitsArchived.mockImplementation(async (_provider: AgentId, ids: string[]) => { for (const id of ids) archived.delete(id); });
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (2)" }));
    const unarchive = screen.getByRole("button", { name: "Unarchive old@muse.example" });
    unarchive.focus();

    fireEvent.click(unarchive);

    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith("muse", ["muse-old"], false));
    expect(await screen.findByRole("region", { name: "Muse limits · old@muse.example" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Archived (1)" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Unarchive spare@muse.example" })).toHaveFocus());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("removes an archived account as Remove account does: the saved login, then its history first", async () => {
    const profile = { id: "saved", observationId: "profile:user-team", legacyId: "team", identity: { provider: "codex" as const, userId: "user", workspaceId: "team" }, email: "shared@example.com", label: "shared@example.com", savedAt: NOW, active: false, loginState: { state: "usable" as const }, archived: true };
    let history = [okCodex(), okCodex({ account: { id: profile.observationId, label: profile.email }, currentAccount: false, archived: true, standing: REMEMBERED,
      profile, replaced: [{ id: "team", label: profile.email }] })];
    readLimits.mockImplementation(async (provider: AgentId) => provider === "codex" ? history : provider === "claude" ? [okClaude()] : []);
    readAccounts.mockImplementation(async (provider: AgentId) => ({ profiles: provider === "codex" ? [profile] : [], nativeAccount: null, recoveryRequired: false, notice: null }));
    forgetLimitsSnapshot.mockImplementation(async (_provider: string, id: string) => { history = history.filter(entry => entry.account?.id !== id); });
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));

    fireEvent.click(screen.getByRole("button", { name: "Remove account shared@example.com" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    await waitFor(() => expect(forgetLimitsSnapshot).toHaveBeenCalledTimes(2));
    expect(accountAction).toHaveBeenCalledWith("codex", "remove", "saved");
    expect(forgetLimitsSnapshot.mock.calls).toEqual([["codex", "team", "shared@example.com"], ["codex", "profile:user-team", undefined]]);
    expect(setLimitsArchived).not.toHaveBeenCalled();
  });

  it.each(["pending", "failed", "recovering"] as const)("keeps an archived row's actions disabled while the account list is %s", async (state) => {
    const denied = { kind: "message", message: "Could not unlock saved accounts.", path: null };
    readAccounts.mockImplementation((provider: AgentId) => state === "pending" ? new Promise(() => {})
      : state === "failed" ? Promise.reject(denied)
      : Promise.resolve({ profiles: [], nativeAccount: null, recoveryRequired: provider === "codex", notice: null }));
    answer([okClaude()], [okCodex(), staleCodex({ savedProfile: true, archived: true })]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    if (state === "failed") await waitFor(() => expect(screen.getAllByText("Could not unlock saved accounts.").length).toBeGreaterThan(0));
    if (state === "recovering") await screen.findByRole("button", { name: "Recover account" });

    expect(screen.getByRole("button", { name: "Unarchive personal@codex.example" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove account personal@codex.example" })).toBeDisabled();
  });

  function answerLapsed(others: LimitsColumnEntry[] = []) {
    const profile = { id: "lapsed", observationId: "acct-personal", legacyId: "team", identity: { provider: "codex" as const, userId: "lapsed", workspaceId: "team" }, email: "personal@codex.example", label: "personal@codex.example", savedAt: NOW, active: false, loginState: { state: "usable" as const } };
    const lapsed = staleCodex({ savedProfile: true, subscription: { activeUntil: "2026-08-10T00:00:00Z", willRenew: false, checkedAt: NOW } });
    answer([okClaude()], withSavedProfiles([okCodex(), lapsed, ...others], [profile]));
    readAccounts.mockImplementation(async (provider: AgentId) => ({ profiles: provider === "codex" ? [profile] : [], nativeAccount: null, recoveryRequired: false, notice: null }));
  }

  it("offers Archive account in place of Use account once a saved subscription ended without renewing", async () => {
    answerLapsed();
    renderLimits();
    const region = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    const footer = await within(region).findByRole("button", { name: "Archive account" });
    expect(within(region).queryByRole("button", { name: "Use account" })).toBeNull();

    footer.focus();
    fireEvent.click(footer);

    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith("codex", ["acct-personal"], true));
    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    const disclosure = screen.getByRole("button", { name: "Archived (1)" });
    expect(disclosure, "focus follows the card instead of dropping to the page").toHaveFocus();
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
  });

  it.each(["menu", "footer"] as const)("leaves focus where it was when archiving from the %s fails", async (from) => {
    setLimitsArchived.mockRejectedValue({ kind: "message", message: "disk full", path: null });
    answerLapsed([staleCodex({ account: { id: "acct-old", label: "old@codex.example" }, archived: true })]);
    renderLimits();
    const region = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    const footer = await within(region).findByRole("button", { name: "Archive account" });
    const trigger = within(region).getByRole("button", { name: "More actions for personal@codex.example" });
    if (from === "footer") {
      footer.focus();
      fireEvent.click(footer);
    } else {
      fireEvent.click(trigger);
      fireEvent.click(within(screen.getByRole("group", { name: "Actions for personal@codex.example" })).getByRole("button", { name: "Archive account" }));
    }
    const focused = from === "footer" ? footer : trigger;
    expect(focused, "the menu hands focus back to its button as it closes").toHaveFocus();

    expect(await screen.findByText("Could not archive that account: disk full")).toBeInTheDocument();
    expect(focused).toHaveFocus();
    expect(screen.getByRole("button", { name: "Archived (1)" })).toBeInTheDocument();
  });

  it("offers Archive account nothing more while it runs, and again once it fails", async () => {
    const archiving = deferred<void>();
    setLimitsArchived.mockReturnValue(archiving.promise);
    answerLapsed();
    renderLimits();
    const region = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    const footer = await within(region).findByRole("button", { name: "Archive account" });

    footer.focus();
    fireEvent.click(footer);
    expect(footer).toHaveAttribute("aria-disabled", "true");
    expect(footer).toHaveAttribute("aria-busy", "true");
    expect(footer).toBeEnabled();
    fireEvent.click(footer);
    fireEvent.click(within(region).getByRole("button", { name: "More actions for personal@codex.example" }));
    const item = within(screen.getByRole("group", { name: "Actions for personal@codex.example" })).getByRole("button", { name: "Archive account" });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAttribute("aria-busy", "true");
    fireEvent.click(item);
    expect(setLimitsArchived, "a second click never sends it again").toHaveBeenCalledTimes(1);

    await act(async () => { archiving.reject({ kind: "message", message: "disk full", path: null }); });

    expect(await screen.findByText("Could not archive that account: disk full")).toBeInTheDocument();
    for (const button of [footer, item]) {
      expect(button).not.toHaveAttribute("aria-disabled");
      expect(button).not.toHaveAttribute("aria-busy");
    }
    fireEvent.click(footer);
    expect(setLimitsArchived, "offered again").toHaveBeenCalledTimes(2);
  });
});
