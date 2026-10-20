import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { LimitsColumnEntry } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { Limits } from "./Limits";
import { NOW, okClaude, okCodex, okMuse, staleCodex, withSavedProfiles } from "./readingFixtures";

const savedAccounts = vi.hoisted(() => ({ codex: true }));
vi.mock("$lib/providers", async (importOriginal) => {
  const real = await importOriginal<typeof import("$lib/providers")>();
  return { ...real, hasSavedAccounts: (id: AgentId) => (id === "codex" ? savedAccounts.codex : real.hasSavedAccounts(id)) };
});

const readLimits = vi.hoisted(() => vi.fn());
const readAccounts = vi.hoisted(() => vi.fn());
const accountAction = vi.hoisted(() => vi.fn());
const readAccountActivationBlockers = vi.hoisted(() => vi.fn());
const pendingResetSpends = vi.hoisted(() => vi.fn());
const addAccount = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, accountAction, readAccountActivationBlockers, pendingResetSpends,
  readAccountPreferences: vi.fn().mockResolvedValue(false), addAccount, cancelAccountLogin: vi.fn(),
  removeLimitsAccount: vi.fn(), setLimitsAccountArchived: vi.fn(), setAccountCategory: vi.fn(),
  consumeCodexResetCredit: vi.fn(), cancelResetSpend: vi.fn(),
  onSharedReadChanged: () => Promise.resolve(() => {}), readCodexSubscription: vi.fn().mockResolvedValue(null),
}));

const DUE = "2026-08-17T20:08:00Z";
const CANCEL_SPEND = "Cancel the automatic banked reset";

function personal(): SavedProfile {
  return {
    id: "saved-personal", observationId: "acct-personal", legacyId: "personal", identity: { provider: "codex", userId: "personal", workspaceId: "personal" },
    email: "personal@codex.example", label: "personal@codex.example", savedAt: NOW, active: false, loginState: { state: "usable" },
  };
}

function answer(codex: LimitsColumnEntry[], profiles: SavedProfile[] = []) {
  readLimits.mockImplementation((provider: AgentId) => Promise.resolve(provider === "claude" ? [okClaude()] : provider === "codex" ? withSavedProfiles(codex, profiles) : [okMuse()]));
  readAccounts.mockImplementation((provider: AgentId) => Promise.resolve({
    profiles: provider === "codex" ? profiles : [], nativeObservationId: provider === "codex" ? "acct-work" : null,
    nativeAccount: null, recoveryRequired: false, notice: null,
  }));
}

function renderLimits() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  render(<QueryClientProvider client={client}><Limits resetAlerts={{}} onResetAlertsChange={async () => undefined} /></QueryClientProvider>);
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  savedAccounts.codex = true;
  readLimits.mockReset();
  readAccounts.mockReset();
  accountAction.mockReset().mockResolvedValue(undefined);
  readAccountActivationBlockers.mockReset().mockResolvedValue([]);
  pendingResetSpends.mockReset().mockResolvedValue([]);
  addAccount.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("a card's banked reset controls", () => {
  it("are offered where the provider keeps no saved accounts, with no account manager to ask", async () => {
    savedAccounts.codex = false;
    answer([okCodex({ resetCredits: { availableCount: 1, nextExpiresAt: null } })]);
    pendingResetSpends.mockResolvedValue([{ accountId: "acct-work", dueAt: DUE }]);
    renderLimits();
    const card = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
    expect(await within(card).findByRole("button", { name: "Use banked reset" })).toBeEnabled();
    expect(await within(card).findByRole("button", { name: CANCEL_SPEND })).toBeEnabled();
    expect(within(card).queryByRole("button", { name: "Save account" })).toBeNull();
  });

  it("ask before using a reset with the card's own countdown to when its limit renews by itself", async () => {
    answer([okCodex({ resetCredits: { availableCount: 1, nextExpiresAt: null } })]);
    renderLimits();
    const card = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
    const use = await within(card).findByRole("button", { name: "Use banked reset" });
    await waitFor(() => expect(use).toBeEnabled());
    fireEvent.click(use);
    expect(screen.getByRole("alertdialog", { name: "Use this reset?" })).toHaveAccessibleDescription(/ and renews by itself in 7d 3h\./);
  });

  it("keep a waiting automatic reset's countdown and Cancel while Switch anyway is asked on the same card", async () => {
    answer([okCodex(), staleCodex()], [personal()]);
    pendingResetSpends.mockResolvedValue([{ accountId: "acct-personal", dueAt: DUE }]);
    readAccountActivationBlockers.mockResolvedValue(["ChatGPT"]);
    renderLimits();
    const card = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    await within(card).findByRole("button", { name: CANCEL_SPEND });
    fireEvent.click(await within(card).findByRole("button", { name: "Use account" }));
    await within(card).findByRole("group", { name: "Confirm switching while Codex is running" });
    expect(within(card).getByRole("button", { name: CANCEL_SPEND })).toBeEnabled();
    expect(within(card).getByText(/^Using a banked reset at/)).toBeTruthy();
    await waitFor(() => expect(accountAction).not.toHaveBeenCalled());
  });
});

describe("a card's footer button", () => {
  it("saves the signed-in login it shows once the account list names it", async () => {
    answer([okCodex()]);
    renderLimits();
    const card = await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
    const save = await within(card).findByRole("button", { name: "Save account" });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(accountAction).toHaveBeenCalledWith("codex", "save", undefined));
    expect(addAccount).not.toHaveBeenCalled();
  });

  it("signs in to an account it has no saved login for, and to a saved login's own profile when that needs it", async () => {
    answer([okCodex(), staleCodex(), staleCodex({ account: { id: "acct-spare", label: "spare@codex.example" } })], [{ ...personal(), observationId: "acct-spare", email: "spare@codex.example", label: "spare@codex.example", loginState: { state: "missing" } }]);
    renderLimits();
    const history = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    fireEvent.click(await within(history).findByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(addAccount).toHaveBeenLastCalledWith("codex", expect.any(String), undefined));
    const saved = screen.getByRole("region", { name: "Codex limits · spare@codex.example" });
    const signIn = within(saved).getByRole("button", { name: "Sign in" });
    await waitFor(() => expect(signIn).toBeEnabled());
    fireEvent.click(signIn);
    await waitFor(() => expect(addAccount).toHaveBeenLastCalledWith("codex", expect.any(String), "saved-personal"));
    expect(accountAction).not.toHaveBeenCalled();
  });
});
