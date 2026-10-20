import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { LimitsColumnEntry } from "$lib/limitsTypes";
import type { LimitsProvider } from "$lib/providers";
import type { AgentId } from "$lib/types";
import { SharedReads } from "$lib/sharedReads/announcements";
import { Limits } from "./Limits";
import { NOW, REMEMBERED, okClaude, okCodex, staleCodex, statusOnly, withSavedProfiles } from "./readingFixtures";

const readAccounts = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const removeLimitsAccount = vi.hoisted(() => vi.fn());
const setLimitsAccountArchived = vi.hoisted(() => vi.fn());
const accountAction = vi.hoisted(() => vi.fn());
const forgetLimitsSnapshot = vi.hoisted(() => vi.fn());
const setLimitsArchived = vi.hoisted(() => vi.fn());
const sharedReadHandlers = vi.hoisted(() => new Set<(change: { source: string }) => void>());
const onSharedReadChanged = vi.hoisted(() => (handler: (change: { source: string }) => void) => {
  sharedReadHandlers.add(handler);
  return Promise.resolve(() => { sharedReadHandlers.delete(handler); });
});

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, removeLimitsAccount, setLimitsAccountArchived, accountAction, forgetLimitsSnapshot, setLimitsArchived, onSharedReadChanged,
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  addAccount: vi.fn().mockResolvedValue(undefined), cancelAccountLogin: vi.fn(), setAccountCategory: vi.fn(), readCodexSubscription: vi.fn().mockResolvedValue(null),
  consumeCodexResetCredit: vi.fn(), pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn(),
}));

type Deferred<T> = { promise: Promise<T>; resolve: (value: T) => void; reject: (reason: unknown) => void };

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

let columns: Record<LimitsProvider, LimitsColumnEntry[]>;

function announce(source: string) {
  for (const handler of sharedReadHandlers) handler({ source });
}

function backendRemoves(provider: LimitsProvider, accountId: string) {
  columns[provider] = columns[provider].filter(entry => entry.currentAccount || entry.account?.id !== accountId);
  announce(`limits:${provider}`);
}

function backendArchives(provider: LimitsProvider, accountId: string, archived: boolean) {
  columns[provider] = columns[provider].map(entry => !entry.currentAccount && entry.account?.id === accountId ? { ...entry, archived } : entry);
  announce(`limits:${provider}`);
}

function serve(codex: LimitsColumnEntry[], claude: LimitsColumnEntry[] = [okClaude()], muse: LimitsColumnEntry[] = []) {
  columns = { claude, codex, muse };
}

function savedLogins(profiles: SavedProfile[]) {
  readAccounts.mockImplementation(async (provider: AgentId) => ({ profiles: provider === "codex" ? profiles : [], nativeAccount: null, recoveryRequired: false, notice: null }));
}

function renderLimits() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  render(
    <QueryClientProvider client={client}>
      <SharedReads />
      <Limits resetAlerts={{}} onResetAlertsChange={async () => undefined} />
    </QueryClientProvider>,
  );
  return client;
}

function card(name: string) {
  return screen.getByRole("region", { name });
}

function expectOneCall(called: typeof removeLimitsAccount, args: unknown[]) {
  expect(called.mock.calls).toEqual([args]);
  for (const other of [removeLimitsAccount, setLimitsAccountArchived, accountAction, forgetLimitsSnapshot, setLimitsArchived]) {
    if (other !== called) expect(other).not.toHaveBeenCalled();
  }
}

const SHARED: SavedProfile = {
  id: "saved", observationId: "profile:user-team", legacyId: "team", identity: { provider: "codex", userId: "user", workspaceId: "team" },
  email: "shared@example.com", label: "shared@example.com", savedAt: NOW, active: false, loginState: { state: "usable" },
};

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readAccounts.mockReset().mockResolvedValue({ profiles: [], nativeAccount: null, recoveryRequired: false, notice: null });
  readLimits.mockReset().mockImplementation(async (provider: LimitsProvider) => columns[provider] ?? []);
  removeLimitsAccount.mockReset().mockImplementation(async (provider: LimitsProvider, accountId: string) => backendRemoves(provider, accountId));
  setLimitsAccountArchived.mockReset().mockImplementation(async (provider: LimitsProvider, accountId: string, archived: boolean) => backendArchives(provider, accountId, archived));
  for (const old of [accountAction, forgetLimitsSnapshot, setLimitsArchived]) old.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("removing", () => {
  it("removes a remembered account with one call and drops its card once the backend announces it", async () => {
    const pending = deferred<void>();
    removeLimitsAccount.mockImplementation(async (provider: LimitsProvider, accountId: string) => { await pending.promise; backendRemoves(provider, accountId); });
    serve([okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());

    fireEvent.click(screen.getByLabelText("More actions for personal@codex.example"));
    const remove = screen.getByRole("button", { name: "Remove account" });
    await waitFor(() => expect(remove).toBeEnabled());
    fireEvent.click(remove);
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    await waitFor(() => expect(removeLimitsAccount).toHaveBeenCalled());
    expect(card("Codex limits · personal@codex.example"), "the UI removes nothing of its own").toBeTruthy();
    await act(async () => { pending.resolve(); });
    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    expect(card("Codex limits · work@codex.example")).toBeTruthy();
    expectOneCall(removeLimitsAccount, ["codex", "acct-personal"]);
  });

  it("removes a saved account, its login and the history it replaced with one call that names the card alone", async () => {
    serve([okCodex(), okCodex({ account: { id: SHARED.observationId, label: SHARED.email }, currentAccount: false, standing: REMEMBERED, profile: SHARED, replaced: [{ id: "team", label: SHARED.email }] })]);
    savedLogins([SHARED]);
    renderLimits();
    await screen.findByRole("button", { name: "Use account" });

    fireEvent.click(screen.getByRole("button", { name: "More actions for shared@example.com" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
    expect(removeLimitsAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · shared@example.com" })).toBeNull());
    expectOneCall(removeLimitsAccount, ["codex", "profile:user-team"]);
  });

  it("keeps the card and reports the one error the removal returns", async () => {
    removeLimitsAccount.mockRejectedValue({ kind: "message", message: "The saved login was removed, but its usage history was not: disk full", path: null });
    serve([okCodex(), staleCodex()]);
    renderLimits();
    await waitFor(() => expect(card("Codex limits · personal@codex.example")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("More actions for personal@codex.example"));
    const remove = screen.getByRole("button", { name: "Remove account" });
    await waitFor(() => expect(remove).toBeEnabled());
    fireEvent.click(remove);
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    expect(await screen.findByText("Could not remove that account: The saved login was removed, but its usage history was not: disk full")).toBeInTheDocument();
    expect(card("Codex limits · personal@codex.example")).toBeTruthy();
    expectOneCall(removeLimitsAccount, ["codex", "acct-personal"]);
  });

  it("removes an archived account with the same one call", async () => {
    serve([okCodex(), okCodex({ account: { id: SHARED.observationId, label: SHARED.email }, currentAccount: false, archived: true, standing: REMEMBERED,
      profile: { ...SHARED, archived: true }, replaced: [{ id: "team", label: SHARED.email }] })]);
    savedLogins([{ ...SHARED, archived: true }]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove account shared@example.com" })).toBeEnabled());

    fireEvent.click(screen.getByRole("button", { name: "Remove account shared@example.com" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    await waitFor(() => expect(screen.queryByRole("button", { name: /^Archived/ })).toBeNull());
    expectOneCall(removeLimitsAccount, ["codex", "profile:user-team"]);
  });
});

describe("archiving", () => {
  it("archives a card from its menu without asking, with one call, and lists it under the column's archived accounts", async () => {
    const spare = staleCodex({ account: { id: "acct-spare", label: "spare@codex.example" } });
    serve([statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.", "muted"), staleCodex(), spare]);
    renderLimits();
    await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    fireEvent.click(screen.getByRole("button", { name: "More actions for me@claude.example" }));
    expect(screen.queryByRole("button", { name: "Archive account" }), "never the signed-in card").toBeNull();
    fireEvent.keyDown(screen.getByRole("button", { name: "More actions for me@claude.example" }), { key: "Escape" });

    fireEvent.click(screen.getByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    const disclosure = screen.getByRole("button", { name: "Archived (1)" });
    expect(disclosure, "focus follows the card instead of dropping to the page").toHaveFocus();
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(card("Codex limits · spare@codex.example"), "only the card archived moves").toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull();
    expectOneCall(setLimitsAccountArchived, ["codex", "acct-personal", true]);
  });

  it("leaves focus where the user moved it while the archive waited", async () => {
    const archiving = deferred<void>();
    setLimitsAccountArchived.mockImplementation(async (provider: LimitsProvider, accountId: string, archived: boolean) => { await archiving.promise; backendArchives(provider, accountId, archived); });
    serve([okCodex(), staleCodex(), staleCodex({ account: { id: "acct-spare", label: "spare@codex.example" } })]);
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

  it("archives and unarchives a saved login no read answered for, by the account its entry names", async () => {
    const unread: SavedProfile = { id: "unread", observationId: "profile:unread", legacyId: "team", identity: { provider: "codex", userId: "unread", workspaceId: "team" }, email: "unread@codex.example", label: "unread@codex.example", savedAt: NOW, active: false, loginState: { state: "usable" } };
    serve(withSavedProfiles([okCodex()], [unread]));
    savedLogins([unread]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "More actions for unread@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · unread@codex.example" })).toBeNull());
    expect(screen.getByRole("button", { name: "Archived (1)" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Archived (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "Unarchive unread@codex.example" }));

    expect(await screen.findByRole("region", { name: "Codex limits · unread@codex.example" })).toBeInTheDocument();
    expect(setLimitsAccountArchived.mock.calls).toEqual([["codex", "profile:unread", true], ["codex", "profile:unread", false]]);
  });

  it("keeps the card and says why when archiving it fails", async () => {
    setLimitsAccountArchived.mockRejectedValue({ kind: "message", message: "disk full", path: null });
    serve([okCodex(), staleCodex()]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "More actions for personal@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));

    expect(await screen.findByText("Could not archive that account: disk full")).toBeInTheDocument();
    expect(card("Codex limits · personal@codex.example")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Archived/ })).toBeNull();
  });

  it("unarchives an account and leaves reading it again to the backend, whose one announcement brings it back", async () => {
    serve([okCodex(), staleCodex({ archived: true })]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    const reads = readLimits.mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: "Unarchive personal@codex.example" }));

    expect(await screen.findByRole("region", { name: "Codex limits · personal@codex.example" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Archived/ })).toBeNull();
    expect(readLimits.mock.calls.slice(reads), "the one read answers the announcement, unforced").toEqual([["codex", false]]);
    expectOneCall(setLimitsAccountArchived, ["codex", "acct-personal", false]);
  });

  it("unarchives an account in the Muse column and hands focus to the next archived row", async () => {
    const weekly = { id: "weekly", label: "Weekly · all models", kind: "weekly" as const, usedPercent: 30, resetsAt: "2026-08-24T20:00:00Z", observedAt: NOW };
    const muse = (id: string, label: string, currentAccount = false, archived = false): LimitsColumnEntry =>
      ({ provider: "muse", status: "ok", account: { id, label }, currentAccount, plan: "wonder", planLabel: "Wonder", windows: [weekly], archived });
    serve([okCodex()], [okClaude()], [muse("muse-now", "now@muse.example", true), muse("muse-old", "old@muse.example", false, true), muse("muse-spare", "spare@muse.example", false, true)]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (2)" }));
    const unarchive = screen.getByRole("button", { name: "Unarchive old@muse.example" });
    await waitFor(() => expect(unarchive).toBeEnabled());
    unarchive.focus();

    fireEvent.click(unarchive);

    expect(await screen.findByRole("region", { name: "Muse limits · old@muse.example" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Archived (1)" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Unarchive spare@muse.example" })).toHaveFocus());
    expect(screen.queryByRole("alert")).toBeNull();
    expectOneCall(setLimitsAccountArchived, ["muse", "muse-old", false]);
  });

  it.each(["pending", "failed", "recovering"] as const)("keeps an archived row's actions disabled while the account list is %s", async (state) => {
    const denied = { kind: "message", message: "Could not unlock saved accounts.", path: null };
    readAccounts.mockImplementation((provider: AgentId) => state === "pending" ? new Promise(() => {})
      : state === "failed" ? Promise.reject(denied)
      : Promise.resolve({ profiles: [], nativeAccount: null, recoveryRequired: provider === "codex", notice: null }));
    serve([okCodex(), staleCodex({ savedProfile: true, archived: true })]);
    renderLimits();
    fireEvent.click(await screen.findByRole("button", { name: "Archived (1)" }));
    if (state === "failed") await waitFor(() => expect(screen.getAllByText("Could not unlock saved accounts.").length).toBeGreaterThan(0));
    if (state === "recovering") await screen.findByRole("button", { name: "Recover account" });

    expect(screen.getByRole("button", { name: "Unarchive personal@codex.example" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove account personal@codex.example" })).toBeDisabled();
  });

  function serveLapsed(others: LimitsColumnEntry[] = []) {
    const profile: SavedProfile = { id: "lapsed", observationId: "acct-personal", legacyId: "team", identity: { provider: "codex", userId: "lapsed", workspaceId: "team" }, email: "personal@codex.example", label: "personal@codex.example", savedAt: NOW, active: false, loginState: { state: "usable" } };
    const lapsed = staleCodex({ savedProfile: true, subscription: { activeUntil: "2026-08-10T00:00:00Z", willRenew: false, checkedAt: NOW } });
    serve(withSavedProfiles([okCodex(), lapsed, ...others], [profile]));
    savedLogins([profile]);
  }

  it("offers Archive account in place of Use account once a saved subscription ended without renewing", async () => {
    serveLapsed();
    renderLimits();
    const region = await screen.findByRole("region", { name: "Codex limits · personal@codex.example" });
    const footer = await within(region).findByRole("button", { name: "Archive account" });
    expect(within(region).queryByRole("button", { name: "Use account" })).toBeNull();

    footer.focus();
    fireEvent.click(footer);

    await waitFor(() => expect(screen.queryByRole("region", { name: "Codex limits · personal@codex.example" })).toBeNull());
    const disclosure = screen.getByRole("button", { name: "Archived (1)" });
    expect(disclosure, "focus follows the card instead of dropping to the page").toHaveFocus();
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expectOneCall(setLimitsAccountArchived, ["codex", "acct-personal", true]);
  });

  it.each(["menu", "footer"] as const)("leaves focus where it was when archiving from the %s fails", async (from) => {
    setLimitsAccountArchived.mockRejectedValue({ kind: "message", message: "disk full", path: null });
    serveLapsed([staleCodex({ account: { id: "acct-old", label: "old@codex.example" }, archived: true })]);
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
    setLimitsAccountArchived.mockReturnValue(archiving.promise);
    serveLapsed();
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
    expect(setLimitsAccountArchived, "a second click never sends it again").toHaveBeenCalledTimes(1);

    await act(async () => { archiving.reject({ kind: "message", message: "disk full", path: null }); });

    expect(await screen.findByText("Could not archive that account: disk full")).toBeInTheDocument();
    for (const button of [footer, item]) {
      expect(button).not.toHaveAttribute("aria-disabled");
      expect(button).not.toHaveAttribute("aria-busy");
    }
    fireEvent.click(footer);
    expect(setLimitsAccountArchived, "offered again").toHaveBeenCalledTimes(2);
  });
});
