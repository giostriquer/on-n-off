import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
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

vi.mock("$lib/api", () => ({ readLimits, readAccounts, accountAction, readAccountPreferences:vi.fn().mockResolvedValue(false), readAccountActivationBlockers:vi.fn().mockResolvedValue([]), addAccount, cancelAccountLogin:vi.fn(), connectCodexBilling:vi.fn(), forgetLimitsSnapshot, onSharedReadChanged, readCodexSubscription: vi.fn().mockResolvedValue({metadata:null,connected:false,unavailable:false}), consumeCodexResetCredit }));

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
      { id: "extra:base_model_inference", label: "Weekly · GPT-RESERVE", kind: "model", usedPercent: 0, resetsAt: "2026-08-24T23:34:33Z", observedAt: NOW },
      { id: "extra:codex_bengalfox", label: "Weekly · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 3, resetsAt: "2026-08-17T19:59:00Z", observedAt: NOW },
      { id: "extra:codex_bengalfox:secondary", label: "5 hour · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 0, resetsAt: "2026-08-17T19:59:00Z", observedAt: NOW },
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
  it.each([
    [0, null, "Starts with your first message"],
    [17, null, "Reset time unavailable"],
    [0, "invalid", "Reset time unavailable"],
  ] as const)("presents a remembered Claude session at %s percent with reset %s", async (usedPercent, resetsAt, expectedNote) => {
    const remembered = okClaude({ currentAccount: false });
    remembered.windows[1] = { ...remembered.windows[1], usedPercent, resetsAt };
    answer([remembered], []);
    renderLimits();

    const region = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const meter = within(region).getByRole("meter", { name: "5 hour · all models" });
    const row = meter.parentElement!;
    expect(meter.getAttribute("aria-valuenow")).toBe(String(usedPercent));
    expect(within(row).getByText(`${usedPercent}%`)).toBeTruthy();
    expect(within(row).getByText(expectedNote)).toBeTruthy();
    expect(within(row).queryByText("—")).toBeNull();
    expect(within(row).queryByText(/resets in/i)).toBeNull();
  });

  it("keeps a reported countdown even when a remembered Claude session has zero usage", async () => {
    const remembered = okClaude({ currentAccount: false });
    remembered.windows[1] = { ...remembered.windows[1], usedPercent: 0, resetsAt: "2026-08-17T23:00:00Z" };
    answer([remembered], []);
    renderLimits();

    const region = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const row = within(region).getByRole("meter", { name: "5 hour · all models" }).parentElement!;
    expect(within(row).getByText("0%")).toBeTruthy();
    expect(within(row).getByText(/^resets in 3h 0m/)).toBeTruthy();
    expect(within(row).queryByText("Reset time unavailable")).toBeNull();
  });

  it("marks only the selected accounts with an accessible dot beside the main window label", async () => {
    answer([okClaude({plan: "max ×5"})], [okCodex(), staleCodex()]);
    renderLimits();
    await screen.findByRole("region", {name: "Claude limits · me@claude.example"});
    for (const name of ["Claude limits · me@claude.example", "Codex limits · work@codex.example"]) {
      const region = card(name);
      const indicator = within(region).getByRole("img", {name: "Active account"});
      expect(within(indicator.parentElement!).getByText("Weekly · all models")).toBeTruthy();
      expect(within(region).queryByText("ACTIVE")).toBeNull();
    }
    expect(within(card("Codex limits · personal@codex.example")).queryByRole("img", {name: "Active account"})).toBeNull();
    expect(within(card("Claude limits · me@claude.example")).getByText("Max ×5")).toBeTruthy();
  });

  it("opens account actions from the header and dismisses them with Escape or an outside click", async () => {
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    const trigger = await screen.findByRole("button", {name: "More actions for personal@codex.example"});
    expect(trigger.closest("header")).not.toBeNull();
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

  it("hides internal reserve and Codex Spark windows while keeping other Codex model limits", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits();

    const codex = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
    expect(within(codex).queryByText(/GPT-RESERVE/i)).toBeNull();
    expect(within(codex).queryByText(/GPT-5\.3-Codex-Spark/i)).toBeNull();
    expect(within(codex).getByRole("meter", { name: "Weekly · GPT-5.6-Luna" })).toBeTruthy();
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

  it("shows remembered accounts after the current one with independently dated windows", async () => {
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());

    const regions = screen.getAllByRole("region").map((region) => region.getAttribute("aria-label"));
    expect(regions.indexOf("Codex limits · work@codex.example")).toBeLessThan(regions.indexOf("Codex limits · personal@codex.example"));

    const stale = card("Codex limits · personal@codex.example");
    expect(stale.getAttribute("data-current-account")).toBe("false");
    expect(within(stale).getByText("Plus")).toBeTruthy();
    expect(within(stale).getByRole("button", { name: "Sign in" })).toBeTruthy();
    // Weekly window's reset is still ahead: numbers stand, with the countdown.
    const weekly = within(stale).getByRole("meter", { name: "Weekly · all models" });
    expect(weekly.getAttribute("aria-valuenow")).toBe("88");
    expect(within(stale).getByText(/resets in 2d 14h/)).toBeTruthy();
    // Session window already reset since the snapshot: no stale percentage is shown.
    const session = within(stale).getByRole("meter", { name: "5 hour · all models" });
    expect(session.getAttribute("aria-valuenow")).toBe("0");
    expect((session.firstElementChild as HTMLElement).style.backgroundColor).toBe("var(--silkscreen)");
    expect(within(stale).getByText("0%").style.color).toBe("");
    // 88 % is inside the band but not spent, so the figure keeps the page ink; the bar carries
    // the signal. It must never be the old amber.
    expect(within(stale).getByText("88%").style.color).toBe("");
    expect(within(stale).getByText(/^reset 22h ago · \w{3} \d\d:\d\d$/)).toBeTruthy();
    expect(within(stale).queryByText(/Current usage unknown/)).toBeNull();
    expect(within(stale).getByLabelText("More actions for personal@codex.example")).toBeTruthy();
    expect(within(stale).getAllByText(/Latest observation/)).toHaveLength(1);
  });

  it("presents an elapsed hero window as reset, never as its old 97% in red", async () => {
    const renewed = staleCodex();
    renewed.windows[0] = { ...renewed.windows[0], usedPercent: 97, resetsAt: "2026-08-17T18:35:00Z" };
    answer([okClaude()], [okCodex(), renewed]);
    renderLimits();

    const stale = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    const weekly = within(stale).getByRole("meter", { name: "Weekly · all models" });
    expect(weekly.getAttribute("aria-valuenow")).toBe("0");
    expect((weekly.firstElementChild as HTMLElement).style.backgroundColor).toBe("var(--silkscreen)");
    expect(weekly.getAttribute("aria-valuetext")).toBeNull();
    expect((weekly.firstElementChild as HTMLElement).style.backgroundColor).not.toBe("var(--trip)");
    const [hero] = within(stale).getAllByText("0%");
    expect(hero.style.color).toBe("");
    expect(within(stale).getByText(/^reset 1h ago · \w{3} \d\d:\d\d$/)).toBeTruthy();
    // The 97% it held before the reset is not recited anywhere on the card.
    expect(within(stale).queryByText(/97/)).toBeNull();
  });

  it("keeps one source-neutral Claude account when current refresh is paused", async () => {
    // What the backend sends when the CLI's access token has gone stale: the endpoint read failed, so
    // the account's last remembered numbers ride along on the same entry.
    answer(
      [
        okClaude({
          status: "unauthenticated",
          message: "Access token expired — send a prompt with `claude` to renew it, then refresh here.",
          windows: [
            { id: "weekly_all", label: "Weekly · all models", kind: "weekly", usedPercent: 12, resetsAt: "2026-08-24T13:59:59Z", observedAt: "2026-08-16T21:40:00.000Z" },
            { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 7, resetsAt: "2026-08-17T04:59:59Z", observedAt: "2026-08-16T21:40:00.000Z" },
          ],
        }),
      ],
      [okCodex()],
    );
    renderLimits();
    await waitFor(() => expect(card("Claude limits · me@claude.example")).toBeTruthy());

    const claude = card("Claude limits · me@claude.example");
    expect(screen.getAllByRole("region", { name: /^Claude limits/ })).toHaveLength(1);
    expect(within(claude).getByText("Refresh paused.")).toBeTruthy();
    expect(within(claude).getByText(/^Access token expired/)).toBeTruthy();
    expect(within(claude).queryByText(/Claude Desktop usage/)).toBeNull();
    expect(within(claude).getByRole("meter", { name: "Weekly · all models" }).getAttribute("aria-valuenow")).toBe("12");
    expect(within(claude).getAllByText(/Latest observation/)).toHaveLength(1);
    // A window that has reset since that read shows the reset, not a stale percentage.
    expect(within(claude).getByText(/^reset 15h ago · \w{3} \d\d:\d\d$/)).toBeTruthy();
    expect(within(claude).queryByText(/Current usage unknown/)).toBeNull();
    // It is still the signed-in account: nothing to sign into, nothing to forget.
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

  it("shows one account timestamp for remembered data when no current read is ok", async () => {
    answer([statusOnly("claude", "signedOut", null)], [statusOnly("codex", "signedOut", null), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());
    expect(within(card("Codex limits · personal@codex.example")).getAllByText(/Latest observation/)).toHaveLength(1);
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

  it.each<[LimitsStatus, string]>([
    ["signedOut", "Sign in with `claude` to see subscription limits."],
    ["unauthenticated", "Login expired — run `claude` and sign in again."],
    ["unsupported", "Claude is signed in with an API key."],
    ["failed", "Could not reach the Claude usage service (HTTP 503)."],
  ])("renders the provider message and status for %s", async (status, message) => {
    answer([statusOnly("claude", status, message)], [okCodex()]);
    renderLimits();
    await waitFor(() => expect(within(card("Claude limits")).getByText(message)).toBeTruthy());
    expect(card("Claude limits").getAttribute("data-status")).toBe(status);
    expect(within(card("Claude limits")).queryByRole("meter")).toBeNull();
  });

  it("falls back to generic copy when a non-ok status carries no message", async () => {
    answer([statusOnly("claude", "failed", null)], [statusOnly("codex", "signedOut", null)]);
    renderLimits();
    await waitFor(() => expect(within(card("Claude limits")).getByText("Claude limits are unavailable.")).toBeTruthy());
    expect(within(card("Codex limits")).getByText("Codex limits are unavailable.")).toBeTruthy();
  });

  it("explains an ok answer with no windows and shows unlimited credits", async () => {
    answer([okClaude({ windows: [], plan: null, account: null })], [okCodex({ credits: { balance: "0", unlimited: true } })]);
    renderLimits();
    await waitFor(() => expect(within(card("Claude limits")).getByText("Claude reported no rate-limit windows.")).toBeTruthy());
    expect(within(card("Claude limits")).queryByText("Max")).toBeNull();
    expect(within(card("Codex limits · work@codex.example")).getByRole("definition", { name: "Credits" }).textContent).toBe("Unlimited");
  });

  it("offers a banked reset beside the current Codex account's actions and lists the count as a row", async () => {
    const remembered = { ...staleCodex(), resetCredits: { availableCount: 1, nextExpiresAt: null } };
    const banked = { availableCount: 2, nextExpiresAt: null };
    answer([okClaude({ resetCredits: banked })], [okCodex({ resetCredits: banked }), remembered]);
    renderLimits();

    const current = await waitFor(() => card("Codex limits · work@codex.example"));
    const button = await within(current).findByRole("button", { name: "Use banked reset" });
    expect(button.closest("footer")).toBe(within(current).getByRole("button", { name: "Save account" }).closest("footer"));
    // Like Save account, it waits until the native login is confirmed to be this card's account.
    expect(button).toHaveProperty("disabled", true);
    expect(within(current).getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");

    // Codex spends a reset on whichever account is signed in, so a remembered card only reports its count.
    const other = card("Codex limits · personal@codex.example");
    expect(within(other).getByRole("definition", { name: "Banked resets" }).textContent).toBe("1");
    expect(within(other).queryByRole("button", { name: "Use banked reset" })).toBeNull();
    // Banked resets are a Codex feature, whatever another provider's read carries.
    expect(within(card("Claude limits · me@claude.example")).queryByRole("button", { name: "Use banked reset" })).toBeNull();
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
it.each([true, false])("shows subscription timing through a header badge (usage available: %s)", async (hasUsage) => {
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValueOnce({
    metadata: {date:"2026-10-10T12:00:00Z",kind:"expires",source:"billing",checkedAt:NOW,stale:false},
    connected:false,unavailable:false,
  });
  const codex = okCodex();
  if (!hasUsage) codex.windows = [];
  answer([okClaude()], [codex]);
  renderLimits();
  const badge = await screen.findByRole("button", {name: "Subscription status: No renewal"});
  expect(badge.closest("header")).toHaveTextContent("Pro ×20");
  expect(screen.queryByText(/Expires in/)).toBeNull();
  fireEvent.focus(badge);
  expect(screen.getByRole("tooltip")).toHaveTextContent(/Expires/);
  fireEvent.keyDown(document, {key:"Escape"});
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(readCodexSubscription).toHaveBeenCalledTimes(1);
});

it("shows a paid reset offer on the live Codex card and on no other", async () => {
 answer([okClaude({ resetOffer: { price: { currency: "USD", amountMinorUnits: 800 } } } as never)],
        [okCodex({ resetOffer: { price: { currency: "USD", amountMinorUnits: 800 } } }), staleCodex()]);
 renderLimits();
 const live = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
 expect(within(live).getByRole("definition", { name: "Paid reset" })).toHaveTextContent("$8.00");
 // A remembered card never carries one, and Claude has no such offer to show at all.
 const remembered = screen.getByRole("region", { name: "Codex limits · personal@codex.example" });
 expect(within(remembered).queryByText("Paid reset")).toBeNull();
 expect(within(await screen.findByRole("region", { name: /^Claude limits/ })).queryByText("Paid reset")).toBeNull();
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

it("tells same-email accounts apart by plan without showing their workspace ids", async () => {
 const accounts=[["personal","ws-personal-7f3a","prolite"],["business","ws-business-9c1e","business"]] as const;
 answer([okClaude()], accounts.map(([id,,plan],index)=>okCodex({account:{id:`profile:${id}`,label:"shared@example.com"},currentAccount:index===0,plan})));
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?accounts.map(([id,workspaceId],index)=>({id,observationId:`profile:${id}`,identity:{provider:"codex",userId:"same-user",workspaceId},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:index===0,needsLogin:false})):[],nativeAccount:null,recoveryRequired:false,notice:null}));
 renderLimits();
 await waitFor(()=>expect(screen.getAllByRole("region",{name:"Codex limits · shared@example.com"})).toHaveLength(2));
 const cards=screen.getAllByRole("region",{name:"Codex limits · shared@example.com"});
 expect(cards.map(card=>within(card).queryByText(/^(Pro ×5|Business)$/)?.textContent).sort()).toEqual(["Business","Pro ×5"]);
 for (const card of cards) expect(card.textContent).not.toMatch(/ws-personal-7f3a|ws-business-9c1e/);
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

it("does not retain forced reads when no Limits screen is mounted", async () => {
  answer([okClaude()], [okCodex()]);
  const client = new QueryClient({defaultOptions: {queries: {retry: false}}});
  await refreshLimits(client);
  expect(readLimits.mock.calls.map(call => call[1])).toEqual([true, true]);
  readLimits.mockClear();
  await client.refetchQueries({queryKey: ["limits"], type: "all"});
  expect(readLimits.mock.calls.map(call => call[1])).toEqual([false, false]);
  client.clear();
});

it("keeps explicit reload queued when an invalidation replaces the background check", async () => {
  const first = deferred<ProviderLimits[]>();
  const replacement = deferred<ProviderLimits[]>();
  let reads = 0;
  readLimits.mockImplementation((provider: AgentId, force: boolean) => provider === "claude" && !force ? (++reads === 1 ? first.promise : replacement.promise) : Promise.resolve(provider === "claude" ? [okClaude({windows: []})] : [okCodex()]));
  const client = new QueryClient({defaultOptions: {queries: {retry: false}}});
  const queryKey = ["limits", "claude"];
  client.setQueryData(queryKey, [okClaude()]);
  const background = client.fetchQuery({queryKey, staleTime: 0, queryFn: () => readLimits("claude", false)}).catch(() => {});
  const refreshed = refreshLimits(client);
  const invalidated = client.refetchQueries({queryKey, type: "all"});
  await Promise.resolve();
  first.resolve([okClaude()]);
  replacement.resolve([okClaude()]);
  await Promise.all([background, refreshed, invalidated]);
  expect(readLimits).toHaveBeenCalledWith("claude", true);
  expect(client.getQueryData<ProviderLimits[]>(queryKey)?.[0].windows).toEqual([]);
  client.clear();
});


it("keeps an unverified legacy card beside its saved login", async () => {
 answer([okClaude()], [okCodex({account:{id:"team",label:"shared@example.com"},currentAccount:false})]);
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?[{id:"saved",observationId:"profile:user-team",identity:{provider:"codex",userId:"user",workspaceId:"team"},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:false,needsLogin:false}]:[],nativeAccount:null,recoveryRequired:false,notice:null}));
 renderLimits();
 await screen.findByRole("button",{name:"Use account"});
 // Unverified legacy quotas stay historical until a fresh scoped observation arrives.
 expect(screen.getAllByRole("region",{name:"Codex limits · shared@example.com"})).toHaveLength(2);
});

it("reconciles legacy cards from saved identity when an older app drops the snapshot alias", async () => {
 const profile = {id:"saved",observationId:"profile:user-team",identity:{provider:"codex",userId:"user",workspaceId:"team"},email:"shared@example.com",label:"shared@example.com",savedAt:NOW,active:false,needsLogin:false};
 const scoped = okCodex({account:{id:profile.observationId,label:profile.email},currentAccount:false,windows:[{id:"primary",label:"Weekly · all models",kind:"weekly",usedPercent:0,observedAt:NOW}]});
 const legacy = okCodex({account:{id:"team",label:profile.email},currentAccount:false});
 answer([okClaude()], [scoped, legacy, staleCodex()]);
 readAccounts.mockImplementation(async(provider:string)=>({profiles:provider==="codex"?[profile]:[],nativeAccount:null,recoveryRequired:false,notice:null}));
 const {client} = renderLimits();
 await screen.findByRole("button",{name:"Use account"});
 expect(screen.getAllByRole("region",{name:"Codex limits · shared@example.com"})).toHaveLength(1);
 expect(within(card("Codex limits · shared@example.com")).getByText("0%")).toBeTruthy();
 expect(card("Codex limits · personal@codex.example")).toBeTruthy();
 // A subsequent older-process refresh must not bring the duplicate back.
 await act(async () => { await client.invalidateQueries({queryKey:["limits","codex"]}); });
 expect(screen.getAllByRole("region",{name:"Codex limits · shared@example.com"})).toHaveLength(1);
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
 expect(forgetLimitsSnapshot).toHaveBeenCalledWith("codex","team","shared@example.com");
 await act(async()=>{await client.invalidateQueries({queryKey:["limits","codex"]});});
 expect(screen.queryByRole("region",{name:"Codex limits · shared@example.com"})).toBeNull();
 expect(card("Codex limits · personal@codex.example")).toBeTruthy();
});


it("updates the expiry warning on the local minute clock without rereading billing", async () => {
  vi.useFakeTimers({toFake:["Date", "setInterval", "clearInterval"]});
  vi.setSystemTime(new Date(NOW));
  vi.mocked(readCodexSubscription).mockClear().mockResolvedValueOnce({
    metadata:{date:"2026-08-17T20:00:30Z",kind:"expires",source:"billing",checkedAt:NOW,stale:false},
    connected:false,unavailable:false,
  });
  answer([okClaude()], [okCodex()]);
  renderLimits();
  const badge = await screen.findByRole("button", {name:"Subscription status: No renewal"});
  expect(badge).toHaveClass("subscription-badge--warning");
  await act(async () => { vi.advanceTimersByTime(60_000); });
  expect(badge).toHaveClass("subscription-badge--expired");
  expect(readCodexSubscription).toHaveBeenCalledTimes(1);
});
