import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { Limits } from "./Limits";

const readLimits = vi.hoisted(() => vi.fn());
const forgetLimitsSnapshot = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({ readLimits, forgetLimitsSnapshot }));

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
      { id: "extra:codex_bengalfox", label: "Weekly · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 3, resetsAt: "2026-08-17T19:59:00Z", observedAt: NOW },
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

function renderLimits() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <Limits />
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
  readLimits.mockReset();
  forgetLimitsSnapshot.mockReset();
  forgetLimitsSnapshot.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Limits", () => {
  it("asks for both subscriptions (not forced) and renders every window with percent, tone and reset", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits();

    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro")).toBeTruthy());
    expect(readLimits).toHaveBeenCalledWith("claude", false);
    expect(readLimits).toHaveBeenCalledWith("codex", false);
    expect(readLimits).toHaveBeenCalledTimes(2);

    const claude = card("Claude limits · me@claude.example");
    expect(claude.getAttribute("data-status")).toBe("ok");
    expect(claude.getAttribute("data-current-account")).toBe("true");
    const weekly = within(claude).getByRole("meter", { name: "Weekly · all models" });
    expect(weekly.getAttribute("aria-valuenow")).toBe("12");
    expect(weekly.getAttribute("data-tone")).toBe("calm");
    expect(within(claude).getByText("12%")).toBeTruthy();
    expect(within(claude).getAllByText(/resets in 6d 17h/)).toHaveLength(2);
    expect(within(claude).getByText(/resets in 8h 59m/)).toBeTruthy();
    const opus = within(claude).getByRole("meter", { name: "Weekly · Opus" });
    expect(opus.getAttribute("data-tone")).toBe("trip");
    expect(within(claude).getByText("91%")).toBeTruthy();
    expect(within(claude).getByRole("meter", { name: "5 hour · all models" }).getAttribute("aria-valuenow")).toBe("7");

    const codex = card("Codex limits · work@codex.example");
    expect(within(codex).getByRole("meter", { name: "Weekly · all models" }).getAttribute("data-tone")).toBe("warn");
    expect(within(codex).getByText(/12\.5 credits/)).toBeTruthy();
    // A current-account observation is still historical after its own reset instant passes.
    const spark = within(codex).getByRole("meter", { name: "Weekly · GPT-5.3-Codex-Spark" });
    expect(spark.getAttribute("aria-valuenow")).toBe("0");
    expect(within(codex).getByText("—")).toBeTruthy();
    expect(within(codex).getByText("Current usage unknown")).toBeTruthy();
    expect(within(codex).getAllByText(/Latest observation/)).toHaveLength(1);
    expect(within(claude).getAllByText(/Latest observation/)).toHaveLength(1);
    expect(within(codex).getAllByText(/resets in/)).toHaveLength(1);
    expect(screen.getByText("Subscription limits")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Forget/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Refresh limits" }).hasAttribute("disabled")).toBe(false);
  });

  it("uses the warning tone for high-usage meter fills", async () => {
    answer([okClaude()], [okCodex()]);
    renderLimits();

    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const opus = within(claude).getByRole("meter", { name: "Weekly · Opus" });
    const codex = screen.getByRole("region", { name: "Codex limits · work@codex.example" });
    const weekly = within(codex).getByRole("meter", { name: "Weekly · all models" });

    expect((opus.firstElementChild as HTMLElement).style.background).toBe("var(--trip)");
    expect((weekly.firstElementChild as HTMLElement).style.background).toBe("var(--warn)");
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
    expect(within(stale).getByText(/Remembered account · sign in as personal@codex\.example to refresh/)).toBeTruthy();
    // Weekly window's reset is still ahead: numbers stand, with the countdown.
    const weekly = within(stale).getByRole("meter", { name: "Weekly · all models" });
    expect(weekly.getAttribute("aria-valuenow")).toBe("88");
    expect(within(stale).getByText(/resets in 2d 14h/)).toBeTruthy();
    // Session window already reset since the snapshot: no stale percentage is shown.
    const session = within(stale).getByRole("meter", { name: "5 hour · all models" });
    expect(session.getAttribute("aria-valuenow")).toBe("0");
    expect(session.getAttribute("data-tone")).toBe("calm");
    expect(within(stale).getByText("—")).toBeTruthy();
    expect(within(stale).getByText("Current usage unknown")).toBeTruthy();
    expect(within(stale).getByRole("button", { name: "Forget personal@codex.example" })).toBeTruthy();
    expect(within(stale).getAllByText(/Latest observation/)).toHaveLength(1);
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
    // A window that has reset since that read shows nothing usable, and asks for nothing either.
    expect(within(claude).getByText(/Current usage unknown/)).toBeTruthy();
    // It is still the signed-in account: nothing to sign into, nothing to forget.
    expect(within(claude).queryByText(/sign in/)).toBeNull();
    expect(within(claude).queryByRole("button", { name: /^Forget/ })).toBeNull();
    expect(screen.getByText("Subscription limits")).toBeTruthy();
  });

  it("re-reads a provider whose current read failed on the next window focus, and leaves a good one cached", async () => {
    answer([statusOnly("claude", "unauthenticated", "Access token expired")], [okCodex()]);
    renderLimits();
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(2));

    await act(async () => {
      window.dispatchEvent(new Event("visibilitychange"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    // The CLI renews its own token, so the failed provider is worth reading again at once; Codex
    // answered fine and stays cached for its stale time.
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(3));
    expect(readLimits.mock.calls[2]).toEqual(["claude", false]);
  });

  it("forgets a remembered account on request and drops its card without a refetch", async () => {
    const pending = deferred<void>();
    forgetLimitsSnapshot.mockReturnValue(pending.promise);
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Forget personal@codex.example" }));
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
    expect(screen.queryByText(/^Subscription limits · as of/)).toBeNull();
    expect(within(card("Codex limits · personal@codex.example")).getAllByText(/Latest observation/)).toHaveLength(1);
  });

  it("keeps the card and reports the error when Forget fails", async () => {
    forgetLimitsSnapshot.mockRejectedValue(new Error("locked"));
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Forget personal@codex.example" }));
    await waitFor(() => expect(screen.getByText(/Could not forget that account: locked/)).toBeTruthy());
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
    expect(screen.getByText("Subscription limits")).toBeTruthy();
  });

  it("falls back to generic copy when a non-ok status carries no message", async () => {
    answer([statusOnly("claude", "failed", null)], [statusOnly("codex", "signedOut", null)]);
    renderLimits();
    await waitFor(() => expect(within(card("Claude limits")).getByText("Claude limits are unavailable.")).toBeTruthy());
    expect(within(card("Codex limits")).getByText("Codex limits are unavailable.")).toBeTruthy();
    expect(screen.queryByText(/^Subscription limits · as of/)).toBeNull();
  });

  it("explains an ok answer with no windows and shows unlimited credits", async () => {
    answer([okClaude({ windows: [], plan: null, account: null })], [okCodex({ credits: { balance: "0", unlimited: true } })]);
    renderLimits();
    await waitFor(() => expect(within(card("Claude limits")).getByText("Claude reported no rate-limit windows.")).toBeTruthy());
    expect(within(card("Claude limits")).queryByText("Max")).toBeNull();
    expect(within(card("Codex limits · work@codex.example")).getByText("unlimited credits")).toBeTruthy();
  });

  it("shows a checking state and disables refresh until the first answer arrives", async () => {
    const pending = deferred<ProviderLimits[]>();
    answer(pending.promise, [okCodex()]);
    renderLimits();
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro")).toBeTruthy());
    const claude = card("Claude limits");
    expect(claude.getAttribute("data-status")).toBe("pending");
    expect(within(claude).getByText(/Checking limits/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh limits" }).hasAttribute("disabled")).toBe(true);
    await act(async () => {
      pending.resolve([okClaude()]);
    });
    await waitFor(() => expect(within(card("Claude limits · me@claude.example")).getByText("Max")).toBeTruthy());
    expect(screen.queryByText(/Checking limits/)).toBeNull();
    expect(screen.getByRole("button", { name: "Refresh limits" }).hasAttribute("disabled")).toBe(false);
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
    await waitFor(() => expect(within(card("Codex limits · work@codex.example")).getByText("Pro")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Refresh limits" }));
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
