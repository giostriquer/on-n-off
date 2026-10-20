import { act, render, screen, fireEvent, waitFor, cleanup, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Profiler } from "react";
import { afterEach, it, expect, vi } from "vitest";
import * as api from "$lib/api";
import type { AccountsReading, SavedProfile } from "$lib/accountTypes";
import { accountListQuery } from "$lib/sharedReads/accounts";
import { AccountControllers, AccountManager, useAccountManagement } from "./AccountManager";
import { ActionMenu } from "@/components/ActionMenu";
import { useCardActions } from "@/features/limits/cardActions";
import { limitCards } from "@/features/limits/limitCards";
import { AccountPreferences } from "./AccountPreferences";
vi.mock("$lib/api", () => ({ readAccounts: vi.fn(), readAccountPreferences: vi.fn(), accountAction: vi.fn(), readAccountActivationBlockers: vi.fn(), addAccount: vi.fn(), cancelAccountLogin: vi.fn() }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const identity = { provider: "codex" as const, userId: "user-a", workspaceId: "team" };
const NOW = Date.parse("2026-09-12T12:00:00Z");
function AccountCard({ profile }: { profile: SavedProfile }) {
  const manager = useAccountManagement();
  const entry = { provider: "codex" as const, status: "ok" as const, account: { id: profile.observationId, label: profile.email }, currentAccount: profile.active, windows: [], unread: !profile.active, profile };
  const [card] = limitCards({ provider: "codex", entries: [entry], nativeLogin: manager?.nativeLogin ?? null, now: NOW })!;
  const actions = useCardActions({ provider: "codex", card, now: NOW, checking: false, onArchive: async () => {}, onRemove: async () => {} });
  return <><header><ActionMenu label={profile.email!} items={actions.menu} /></header><footer>{actions.footer}</footer></>;
}
function Cards() {
  const manager = useAccountManagement();
  return <>{manager?.query.data?.profiles.map(profile => <section key={profile.id} aria-label={profile.email ?? "account"}>
    <span>{profile.email}</span><span>{profile.category}</span>
    <AccountCard profile={profile} />
  </section>)}</>;
}
const OTHER: SavedProfile = { id: "profile-b", observationId: "profile:billing-b", legacyId: "other", identity: { ...identity, userId: "user-b" }, label: "other@example.com", email: "other@example.com", active: false, loginState: { state: "usable" }, savedAt: "2026-09-12T12:00:00Z" };
function setup({ preferences = false, onCommit, signedIn = false, others = [] }: { preferences?: boolean; onCommit?: () => void; signedIn?: boolean; others?: SavedProfile[] } = {}) {
  vi.mocked(api.readAccountPreferences).mockResolvedValue(false);
  vi.mocked(api.readAccounts).mockResolvedValue({ profiles: [{ id: "profile-a", observationId: "profile:billing-a", legacyId: "team", identity, label: "Legacy name", email: "person@example.com", category: "Client A", active: signedIn, loginState: { state: "usable" }, savedAt: "2026-09-12T12:00:00Z" }, ...others], ...(signedIn ? { nativeObservationId: "profile:billing-a" } : {}), nativeAccount: null, recoveryRequired: false, notice: null });
  vi.mocked(api.accountAction).mockResolvedValue();
  vi.mocked(api.readAccountActivationBlockers).mockResolvedValue([]);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = <QueryClientProvider client={client}>{preferences ? <AccountPreferences /> : <AccountControllers><AccountManager provider="codex"><Cards /></AccountManager></AccountControllers>}</QueryClientProvider>;
  render(onCommit ? <Profiler id="accounts" onRender={onCommit}>{tree}</Profiler> : tree);
  return client;
}
it("reads accounts without changing login and switches only from the selected card", async () => {
  setup(); const card = await screen.findByRole("region", { name: "person@example.com" });
  expect(api.accountAction).not.toHaveBeenCalled();
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "use", "profile-a"));
  expect(api.readAccountActivationBlockers).toHaveBeenCalledWith("codex");
});
it("asks before switching beside running clients and switches only on confirmation", async () => {
  setup(); vi.mocked(api.readAccountActivationBlockers).mockResolvedValue(["Acme Studio (codex)", "ChatGPT"]);
  const card = await screen.findByRole("region", { name: "person@example.com" });
  const name = "Confirm switching while Codex is running";
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  const confirm = await within(card).findByRole("group", { name });
  expect(confirm).toHaveTextContent("Acme Studio (codex), ChatGPT");
  expect(within(confirm).getByRole("button", { name: "Cancel" })).toHaveFocus();
  fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
  expect(within(card).queryByRole("group", { name })).toBeNull();
  expect(within(card).getByRole("button", { name: "Use account" })).toHaveFocus();
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  fireEvent.keyDown(await within(card).findByRole("group", { name }), { key: "Escape" });
  expect(within(card).queryByRole("group", { name })).toBeNull();
  expect(api.accountAction).not.toHaveBeenCalled();
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  fireEvent.click(await within(card).findByRole("button", { name: "Switch anyway" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "useAlongsideClients", "profile-a"));
  expect(vi.mocked(api.accountAction).mock.calls.map(call => call[1])).toEqual(["useAlongsideClients"]);
  await waitFor(() => expect(within(card).queryByRole("group", { name })).toBeNull());
});
it("moves focus to Cancel in the same commit that shows the running-clients confirmation", async () => {
  const name = "Confirm switching while Codex is running"; let focusedOnShow: Element | null | undefined;
  setup({ onCommit: () => { if (focusedOnShow === undefined && document.querySelector(`[role="group"][aria-label="${name}"]`)) focusedOnShow = document.activeElement; } });
  vi.mocked(api.readAccountActivationBlockers).mockResolvedValue(["ChatGPT"]);
  const card = await screen.findByRole("region", { name: "person@example.com" });
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  const confirm = await within(card).findByRole("group", { name });
  expect(focusedOnShow).toBe(within(confirm).getByRole("button", { name: "Cancel" }));
});
it("switches normally when running clients cannot be checked, and waits for the check", async () => {
  setup(); let answer!: (running: string[]) => void;
  vi.mocked(api.readAccountActivationBlockers).mockImplementationOnce(() => new Promise(resolve => { answer = resolve; }));
  const card = await screen.findByRole("region", { name: "person@example.com" });
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  await waitFor(() => expect(within(card).getByRole("button", { name: "Use account" })).toBeDisabled());
  fireEvent.click(within(card).getByRole("button", { name: "More actions for person@example.com" }));
  expect(await within(card).findByRole("group", { name: "Actions for person@example.com" })).toBeVisible();
  fireEvent.keyDown(within(card).getByRole("group", { name: "Actions for person@example.com" }), { key: "Escape" });
  expect(api.accountAction).not.toHaveBeenCalled();
  await act(async () => answer([]));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "use", "profile-a"));
  vi.mocked(api.accountAction).mockClear();
  vi.mocked(api.readAccountActivationBlockers).mockRejectedValueOnce(new Error("Could not list processes"));
  await waitFor(() => expect(within(card).getByRole("button", { name: "Use account" })).toBeEnabled());
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "use", "profile-a"));
  expect(within(card).queryByRole("group", { name: "Confirm switching while Codex is running" })).toBeNull();
});
it("asks before removing the signed-in account's saved login, which leaves it signed in", async () => {
  setup({ signedIn: true }); await screen.findByText("person@example.com");
  fireEvent.click(screen.getByText("•••"));
  fireEvent.click(screen.getByRole("button", { name: "Remove saved login" }));
  const confirm = screen.getByRole("group", { name: "Confirm account action" });
  expect(confirm).toHaveTextContent("Remove the saved login for person@example.com? This account stays signed in.");
  expect(api.accountAction).not.toHaveBeenCalled();
  fireEvent.click(within(confirm).getByRole("button", { name: "Confirm removal" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "remove", "profile-a"));
  await waitFor(() => expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull());
});
it("keeps email as the account name", async () => {
  setup(); await screen.findByText("person@example.com");
  expect(screen.queryByText("Legacy name")).toBeNull();
});
it("re-reads both account lists, the Codex subscription dates and that provider's limits after an account action, and no other read", async () => {
  const client = setup();
  const card = await screen.findByRole("region", { name: "person@example.com" });
  const others = [["limits", "codex"], ["limits", "claude"], ["subscription", "codex", "profile:billing-a"], ["github", "prs"], ["reset-spends"]];
  for (const key of others) client.setQueryData(key, []);
  expect(vi.mocked(api.readAccounts).mock.calls).toEqual([["claude"], ["codex"], ["muse"]]);
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  await waitFor(() => expect(vi.mocked(api.readAccounts).mock.calls).toEqual([["claude"], ["codex"], ["muse"], ["claude"], ["codex"], ["muse"]]));
  expect(others.map(key => client.getQueryState(key)?.isInvalidated)).toEqual([true, false, true, false, false]);
});
it("re-reads the account lists and the remembering preference, not the subscription dates, when automatic saving changes", async () => {
  const client = setup({ preferences: true });
  const checkbox = await screen.findByRole("button", { name: "Automatically save accounts I sign in to" });
  await waitFor(() => expect(checkbox).toBeEnabled());
  client.setQueryData(["accounts", "codex"], { profiles: [] });
  client.setQueryData(["subscription", "codex", "profile:billing-a"], null);
  fireEvent.click(checkbox);
  await waitFor(() => expect(api.readAccountPreferences).toHaveBeenCalledTimes(2));
  expect(client.getQueryState(["accounts", "codex"])?.isInvalidated).toBe(true);
  expect(client.getQueryState(["subscription", "codex", "profile:billing-a"])?.isInvalidated).toBe(false);
});
function openSignOut(card: HTMLElement) {
  fireEvent.click(within(card).getByRole("button", { name: "More actions for person@example.com" }));
  return within(card).getByRole("button", { name: "Sign out" });
}
it("signs the signed-in account out once confirmed, and closes the confirmation", async () => {
  setup({ signedIn: true });
  const card = await screen.findByRole("region", { name: "person@example.com" });
  const signOut = openSignOut(card);
  await waitFor(() => expect(signOut).toBeEnabled());
  fireEvent.click(signOut);
  fireEvent.click(within(card).getByRole("button", { name: "Confirm sign out" }));
  await waitFor(() => expect(vi.mocked(api.accountAction).mock.calls).toEqual([["codex", "signOut", undefined]]));
  await waitFor(() => expect(within(card).queryByRole("group", { name: "Confirm account action" })).toBeNull());
});
it("says in its confirmation why a confirmed sign-out failed, and lets it be tried again", async () => {
  setup({ signedIn: true });
  const card = await screen.findByRole("region", { name: "person@example.com" });
  const signOut = openSignOut(card);
  await waitFor(() => expect(signOut).toBeEnabled());
  fireEvent.click(signOut);
  vi.mocked(api.accountAction).mockRejectedValueOnce(new Error("the provider refused the sign-out"));
  fireEvent.click(within(card).getByRole("button", { name: "Confirm sign out" }));
  await waitFor(() => expect(within(card).getByRole("alert")).toHaveTextContent("the provider refused the sign-out"));
  expect(within(card).getByRole("group", { name: "Confirm account action" })).toBeInTheDocument();
  await waitFor(() => expect(within(card).getByRole("button", { name: "Confirm sign out" })).toBeEnabled());
});
it("refuses a sign-out confirmed after the cached account list already names another signed-in account, before the card redraws", async () => {
  const client = setup({ signedIn: true });
  const card = await screen.findByRole("region", { name: "person@example.com" });
  const signOut = openSignOut(card);
  await waitFor(() => expect(signOut).toBeEnabled());
  fireEvent.click(signOut);
  const confirm = within(card).getByRole("button", { name: "Confirm sign out" });
  const { queryKey } = accountListQuery("codex");
  client.setQueryData<AccountsReading>(queryKey, (reading) => reading && { ...reading, nativeObservationId: "profile:billing-b" });
  expect(confirm, "the card has not redrawn from the new list yet").toBeEnabled();
  fireEvent.click(confirm);
  expect(within(card).queryByRole("group", { name: "Confirm account action" })).toBeNull();
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  expect(api.accountAction).not.toHaveBeenCalled();
});
it("requires opt-in in Settings before automatically saving accounts", async () => {
  setup({ preferences: true }); const checkbox = await screen.findByRole("button", { name: "Automatically save accounts I sign in to" });
  await waitFor(() => expect(checkbox).toBeEnabled()); expect(checkbox).toHaveAttribute("aria-pressed", "false");
  expect(api.accountAction).not.toHaveBeenCalled(); fireEvent.click(checkbox);
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "remember"));
});
it("can disable automatic saving even when the profile vault cannot be read", async () => {
  vi.mocked(api.readAccountPreferences).mockResolvedValue(true);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  vi.mocked(api.readAccounts).mockRejectedValue(new Error("Vault unavailable"));
  render(<QueryClientProvider client={client}><AccountPreferences /></QueryClientProvider>);
  const checkbox = await screen.findByRole("button", { name: "Automatically save accounts I sign in to" }); await waitFor(() => expect(checkbox).toHaveAttribute("aria-pressed", "true"));
  fireEvent.click(checkbox);
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "stopRemembering"));
});
it("keeps cancellation on the initiating card and permits retry after cleanup", async () => {
  let finish!: () => void;
  vi.mocked(api.addAccount).mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
  vi.mocked(api.cancelAccountLogin).mockResolvedValue();
  setup({ others: [OTHER] });
  const card = await screen.findByRole("region", {name: "person@example.com"});
  const other = screen.getByRole("region", {name: "other@example.com"});
  const othersFooter = () => within(other).getAllByRole("button").filter(button => button.closest("footer")).map(button => [button.textContent, button.hasAttribute("disabled")]);
  fireEvent.click(within(card).getByLabelText("More actions for person@example.com"));
  fireEvent.click(within(card).getByRole("button", {name: "Sign in again"}));
  const cancel = within(card).getByRole("button", {name: "Cancel sign-in"});
  expect(cancel).toBeEnabled();
  expect(othersFooter(), "another card keeps its own action, held while the sign-in runs").toEqual([["Use account", true]]);
  const operation = vi.mocked(api.addAccount).mock.calls.at(-1)![1];
  fireEvent.click(cancel);
  await waitFor(() => expect(api.cancelAccountLogin).toHaveBeenCalledWith(operation));
  expect(within(card).getByRole("button", {name: "Canceling…"})).toBeDisabled();
  expect(othersFooter()).toEqual([["Use account", true]]);
  await act(async () => finish());
  await waitFor(() => expect(within(card).getByRole("button", {name: "Use account"})).toBeEnabled());
  expect(within(card).queryByRole("button", {name: "Cancel sign-in"})).toBeNull();
});

it("cancels an unfinished sign-in when the account screen is left", async () => {
  let finish!: () => void;
  vi.mocked(api.addAccount).mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
  vi.mocked(api.cancelAccountLogin).mockResolvedValue();
  setup();
  const card = await screen.findByRole("region", {name: "person@example.com"});
  fireEvent.click(within(card).getByLabelText("More actions for person@example.com"));
  fireEvent.click(within(card).getByRole("button", {name: "Sign in again"}));
  const operation = vi.mocked(api.addAccount).mock.calls.at(-1)![1];
  cleanup();
  expect(api.cancelAccountLogin).toHaveBeenCalledWith(operation);
  await act(async () => finish());
});

it("retries a denied vault unlock without starting sign-in or changing accounts", async () => {
  const client = setup();
  await screen.findByRole("region", {name: "person@example.com"});
  const restored = vi.mocked(api.readAccounts).getMockImplementation()!;
  vi.mocked(api.readAccounts).mockRejectedValue(new Error("Keychain access denied"));
  await act(async () => { await client.invalidateQueries({queryKey: ["accounts"]}); });
  const retry = await screen.findByRole("button", {name: "Retry account access"});
  vi.mocked(api.accountAction).mockImplementation(async () => {
    vi.mocked(api.readAccounts).mockImplementation(restored);
  });
  fireEvent.click(retry);
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "unlock", undefined));
  await waitFor(() => expect(screen.queryByRole("button", {name: "Retry account access"})).toBeNull());
  expect(screen.getByRole("button", {name: "Use account"})).toBeEnabled();
  expect(api.addAccount).not.toHaveBeenCalled();
});

