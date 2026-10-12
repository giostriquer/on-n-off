import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { Limits } from "./Limits";
import { NOW, okClaude, okCodex } from "./readingFixtures";

const readAccounts = vi.hoisted(() => vi.fn());
const addAccount = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, addAccount, accountAction: vi.fn().mockResolvedValue(undefined),
  forgetLimitsSnapshot: vi.fn().mockResolvedValue(undefined), setLimitsArchived: vi.fn().mockResolvedValue(undefined),
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  cancelAccountLogin: vi.fn().mockResolvedValue(undefined), consumeCodexResetCredit: vi.fn(), pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn(),
  onSharedReadChanged: () => Promise.resolve(() => {}), readCodexSubscription: vi.fn().mockResolvedValue(null),
}));

const DAY = 86_400_000;
const at = (days: number) => new Date(Date.parse(NOW) + days * DAY).toISOString();

function profile(overrides: Partial<SavedProfile>): SavedProfile {
  return {
    id: "saved-me", observationId: "uuid-1", identity: { provider: "claude", userId: "me", workspaceId: "team" },
    label: "me@claude.example", email: "me@claude.example", savedAt: NOW, active: true, needsLogin: false, ...overrides,
  };
}

function answer(claude: ProviderLimits[], profiles: SavedProfile[]) {
  readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? claude : [okCodex()]));
  readAccounts.mockImplementation((provider: AgentId) => Promise.resolve({
    profiles: provider === "claude" ? profiles : [], nativeAccount: null, recoveryRequired: false, notice: null,
  }));
}

function renderLimits() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  render(<QueryClientProvider client={client}><Limits resetAlerts={{}} onResetAlertsChange={async () => undefined} /></QueryClientProvider>);
}

const signedInCard = () => screen.findByRole("region", { name: "Claude limits · me@claude.example" });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readAccounts.mockReset();
  readLimits.mockReset();
  addAccount.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("a Claude sign-in's end", () => {
  it("is not mentioned while more than a week remains", async () => {
    answer([okClaude()], [profile({ signInEndsAt: at(20) })]);
    renderLimits();
    const card = await signedInCard();
    await waitFor(() => expect(within(card).getByRole("button", { name: "More actions for me@claude.example" })).toBeTruthy());
    expect(within(card).queryByRole("button", { name: /sign-in/i })).toBeNull();
  });

  it("is a warning badge in the card header through its last week", async () => {
    answer([okClaude()], [profile({ signInEndsAt: at(3) })]);
    renderLimits();
    const card = await signedInCard();
    const badge = await within(card).findByRole("button", { name: "Claude sign-in ends Aug 20" });
    expect(badge.closest("header")).toBeTruthy();
    expect(badge).toHaveClass("term-badge--warning");
    expect(badge).toHaveTextContent("Sign-in ends Aug 20");
    expect(parseFloat(badge.querySelector<HTMLElement>(".term-badge-fill")!.style.width)).toBeCloseTo(400 / 7, 1);
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("3 days remaining");
    expect(screen.getByRole("tooltip")).toHaveTextContent("Then Claude Code asks you to sign in again.");
    expect(within(card).queryByRole("button", { name: "Sign in again" })).toBeNull();
  });

  it("once passed, replaces the signed-in card's sign-in hint and offers to sign in again", async () => {
    answer(
      [{ ...okClaude({ windows: [] }), status: "signedOut", message: "Sign in with `claude` to see subscription limits." }],
      [profile({ active: false, signInEndsAt: at(-1) })],
    );
    renderLimits();
    const card = await signedInCard();
    expect(await within(card).findByRole("button", { name: "Claude sign-in ended Aug 16" })).toHaveClass("term-badge--expired");
    expect(within(card).getByText("Claude ended this sign-in on Aug 16. Sign in again to use this account.")).toBeTruthy();
    expect(within(card).queryByText(/Sign in with `claude`/)).toBeNull();
    fireEvent.click(within(card).getByRole("button", { name: "Sign in again" }));
    await waitFor(() => expect(addAccount).toHaveBeenCalledWith("claude", expect.any(String), "saved-me"));
  });

  it("once passed, puts signing in again before using a saved account", async () => {
    answer(
      [
        okClaude({ account: { id: "uuid-2", label: "now@claude.example" } }),
        { ...okClaude(), currentAccount: false, savedProfile: true, status: "failed", message: "This account's saved login has ended. Sign in again." },
      ],
      [profile({ active: false, signInEndsAt: at(-2) })],
    );
    renderLimits();
    const card = await signedInCard();
    expect(await within(card).findByText("Claude ended this sign-in on Aug 15. Sign in again to use this account.")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Sign in again" })).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Use account" })).toBeNull();
  });

  it("while signing in again, offers to cancel that sign-in instead", async () => {
    addAccount.mockReturnValue(new Promise(() => {}));
    answer([{ ...okClaude({ windows: [] }), status: "signedOut", message: null }], [profile({ active: false, signInEndsAt: at(-1) })]);
    renderLimits();
    const card = await signedInCard();
    fireEvent.click(await within(card).findByRole("button", { name: "Sign in again" }));
    expect(await within(card).findByRole("button", { name: "Cancel sign-in" })).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Sign in again" })).toBeNull();
  });

  it("asks an account added but never used to sign in again, not to use a login that has ended", async () => {
    answer(
      [{ ...okClaude(), currentAccount: false, savedProfile: true, status: "failed", message: "This account's saved login has ended. Sign in again." }],
      [profile({ active: false, pendingActivation: true, signInEndsAt: at(-1) })],
    );
    renderLimits();
    const card = await signedInCard();
    expect(await within(card).findByRole("button", { name: "Sign in again" })).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Use account" })).toBeNull();
  });

  it("ends on the local minute clock, without reading the account again", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(new Date(NOW));
    answer([okClaude()], [profile({ signInEndsAt: new Date(Date.parse(NOW) + 30_000).toISOString() })]);
    renderLimits();
    const card = await signedInCard();
    expect(await within(card).findByRole("button", { name: "Claude sign-in ends Aug 17" })).toHaveClass("term-badge--warning");
    const reads = readAccounts.mock.calls.length;
    await act(async () => { vi.advanceTimersByTime(60_000); });
    expect(within(card).getByRole("button", { name: "Claude sign-in ended Aug 17" })).toHaveClass("term-badge--expired");
    expect(within(card).getByText("Claude ended this sign-in on Aug 17. Sign in again to use this account.")).toBeTruthy();
    expect(readAccounts.mock.calls.length).toBe(reads);
  });
});
