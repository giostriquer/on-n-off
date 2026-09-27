import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { Limits } from "./Limits";
import { NOW, okClaude, okCodex, staleCodex, statusOnly } from "./readingFixtures";

// Where focus goes on the Limits screen when an account's card or archived row goes away.

const readAccounts = vi.hoisted(() => vi.fn());
const accountAction = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const forgetLimitsSnapshot = vi.hoisted(() => vi.fn());
const setLimitsArchived = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, accountAction, forgetLimitsSnapshot, setLimitsArchived,
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  addAccount: vi.fn().mockResolvedValue(undefined), cancelAccountLogin: vi.fn(), consumeCodexResetCredit: vi.fn(),
  onSharedReadChanged: () => Promise.resolve(() => {}), readCodexSubscription: vi.fn().mockResolvedValue(null),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

function answer(claude: ProviderLimits[], codex: ProviderLimits[]) {
  readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? claude : codex));
}

/** The screen, and a way to render it again as a poll or the minute timer would. */
function renderLimits() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  const screenTree = () => <QueryClientProvider client={client}><Limits /></QueryClientProvider>;
  const view = render(screenTree());
  return { renderAgain: () => view.rerender(screenTree()) };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readAccounts.mockReset().mockResolvedValue({ profiles: [], nativeAccount: null, recoveryRequired: false, notice: null });
  accountAction.mockReset().mockResolvedValue(undefined);
  readLimits.mockReset();
  forgetLimitsSnapshot.mockReset().mockResolvedValue(undefined);
  setLimitsArchived.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("focus as an account leaves", () => {
  const spare = staleCodex({ account: { id: "acct-spare", label: "spare@codex.example" } });
  const signedOut = statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.");

  async function removeFromMenu(label: string) {
    fireEvent.click(await screen.findByRole("button", { name: `More actions for ${label}` }));
    fireEvent.click(within(screen.getByRole("group", { name: `Actions for ${label}` })).getByRole("button", { name: "Remove account" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(screen.queryByRole("region", { name: `Codex limits · ${label}` })).toBeNull());
  }

  it.each([
    ["the next card's More actions", [okCodex(), staleCodex(), spare], "personal@codex.example", "More actions for spare@codex.example"],
    ["the previous card's, from the last card", [okCodex(), staleCodex(), spare], "spare@codex.example", "More actions for personal@codex.example"],
    ["the next card's, not the column's archived list", [okCodex(), staleCodex(), spare, staleCodex({ account: { id: "acct-old", label: "old@codex.example" }, archived: true })],
      "personal@codex.example", "More actions for spare@codex.example"],
    ["Add account, with no other card's actions left", [signedOut, staleCodex()], "personal@codex.example", "Add account"],
  ])("goes to %s when a card is removed from its menu", async (_, codex, label, target) => {
    answer([okClaude()], codex);
    renderLimits();
    await removeFromMenu(label);
    expect(screen.getByRole("button", { name: target })).toHaveFocus();
  });

  it("goes to the archived list when an archive resolves after the user tabbed within its card", async () => {
    const archiving = deferred<void>();
    setLimitsArchived.mockReturnValue(archiving.promise);
    answer([okClaude()], [okCodex(), staleCodex()]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));
    // Tab from the card's ••• to its footer while the archive waits.
    within(screen.getByRole("region", { name: "Codex limits · personal@codex.example" })).getByRole("button", { name: "Sign in" }).focus();

    await act(async () => { archiving.resolve(); });

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    expect(document.body, "the card took focus with it").not.toHaveFocus();
    expect(screen.getByRole("button", { name: "Archived (1)" })).toHaveFocus();
  });

  it("stays where the user moved it while the removal waited", async () => {
    const forgetting = deferred<void>();
    forgetLimitsSnapshot.mockReturnValue(forgetting.promise);
    answer([okClaude()], [okCodex(), staleCodex(), spare]);
    const { renderAgain } = renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    const elsewhere = screen.getByRole("button", { name: "More actions for me@claude.example" });
    elsewhere.focus();

    await act(async () => { forgetting.resolve(); });

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    expect(elsewhere).toHaveFocus();

    act(() => elsewhere.blur());
    renderAgain();
    await act(async () => {});
    expect(document.body, "a hand-off that did not move focus is over, not waiting for a later render").toHaveFocus();
  });

  it("goes to the column's first card once its last archived account is unarchived", async () => {
    answer([okClaude()], [okCodex(), staleCodex({ archived: true })]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    const unarchive = screen.getByRole("button", { name: "Unarchive personal@codex.example" });
    await waitFor(() => expect(unarchive).toBeEnabled());
    unarchive.focus();
    fireEvent.click(unarchive);

    await waitFor(() => expect(screen.queryByRole("button", { name: /^Archived/ })).toBeNull());
    await waitFor(() => expect(screen.getByRole("button", { name: "More actions for work@codex.example" })).toHaveFocus());
  });
});
