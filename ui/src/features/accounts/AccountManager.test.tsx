import { act, render, screen, fireEvent, waitFor, cleanup, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, it, expect, vi } from "vitest";
import * as api from "$lib/api";
import { AccountControllers, AccountManager, useAccountManagement } from "./AccountManager";
import { AccountBilling } from "./AccountBilling";
import { AccountCardActions } from "./AccountCardActions";
import { AccountPreferences } from "./AccountPreferences";
vi.mock("$lib/api", () => ({ readAccounts: vi.fn(), readAccountPreferences: vi.fn(), accountAction: vi.fn(), readAccountActivationBlockers: vi.fn(), addAccount: vi.fn(), cancelAccountLogin: vi.fn(), readCodexSubscription: vi.fn(), connectCodexBilling: vi.fn() }));
vi.mock("$lib/useSharedRead", () => ({ useSharedRead: vi.fn() }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const identity = { provider: "codex" as const, userId: "user-a", workspaceId: "team" };
const forget = vi.fn().mockResolvedValue(undefined);
function Cards() {
  const manager = useAccountManagement();
  return <>{manager?.query.data?.profiles.map(profile => <section key={profile.id} aria-label={profile.email ?? "account"}>
    <span>{profile.email}</span><span>{profile.category}</span>
    <AccountCardActions accountId={profile.observationId!} label={profile.email!} current={profile.active} profile={profile} onForget={forget} header={menu => <header>{menu}</header>}
      footer={({ blocked }) => <AccountBilling accountId={profile.observationId!} disabled={blocked} showDate={false} />} />
  </section>)}</>;
}
function setup(preferences = false) {
  vi.mocked(api.readAccountPreferences).mockResolvedValue(false);
  vi.mocked(api.readAccounts).mockResolvedValue({ profiles: [{ id: "profile-a", observationId: "profile:billing-a", identity, label: "Legacy name", email: "person@example.com", category: "Client A", active: false, needsLogin: false, savedAt: "2026-09-12T12:00:00Z" }], nativeAccount: null, recoveryRequired: false, notice: null });
  vi.mocked(api.accountAction).mockResolvedValue();
  vi.mocked(api.readAccountActivationBlockers).mockResolvedValue([]);
  vi.mocked(api.readCodexSubscription).mockResolvedValue({ metadata: null, connected: false, unavailable: false, browserSupported: true, canConnect: true });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}>{preferences ? <AccountPreferences /> : <AccountControllers><AccountManager provider="codex"><Cards /></AccountManager></AccountControllers>}</QueryClientProvider>);
  return client;
}
it("reads accounts without changing login and switches only from the selected card", async () => {
  setup(); const card = await screen.findByRole("region", { name: "person@example.com" });
  expect(api.accountAction).not.toHaveBeenCalled();
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "use", "profile-a", undefined));
});
it("asks before switching beside running clients and switches only on confirmation", async () => {
  setup(); vi.mocked(api.readAccountActivationBlockers).mockResolvedValue(["Acme Studio (codex)", "ChatGPT"]);
  const card = await screen.findByRole("region", { name: "person@example.com" });
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  const confirm = await within(card).findByRole("group", { name: "Confirm switching while Codex is running" });
  expect(confirm).toHaveTextContent("Acme Studio (codex), ChatGPT");
  expect(within(confirm).getByRole("button", { name: "Cancel" })).toHaveFocus();
  fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
  expect(within(card).queryByRole("group", { name: "Confirm switching while Codex is running" })).toBeNull();
  expect(within(card).getByRole("button", { name: "Use account" })).toHaveFocus();
  expect(api.accountAction).not.toHaveBeenCalled();
  fireEvent.click(within(card).getByRole("button", { name: "Use account" }));
  fireEvent.click(await within(card).findByRole("button", { name: "Switch anyway" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "useAlongsideClients", "profile-a", undefined));
  expect(api.accountAction).not.toHaveBeenCalledWith("codex", "use", expect.anything(), expect.anything());
  await waitFor(() => expect(within(card).queryByRole("group", { name: "Confirm switching while Codex is running" })).toBeNull());
});
it("removes saved login and its card only after confirmation without signing out", async () => {
  setup(); await screen.findByText("person@example.com");
  fireEvent.click(screen.getByText("•••"));
  fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
  expect(api.accountAction).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
  await waitFor(() => expect(forget).toHaveBeenCalledWith("profile:billing-a"));
  expect(api.accountAction).toHaveBeenCalledWith("codex", "remove", "profile-a", undefined);
  expect(api.accountAction).not.toHaveBeenCalledWith("codex", "signOut", expect.anything(), expect.anything());
});
it("keeps email as the account name and allows a free-text category to be edited or cleared", async () => {
  setup(); await screen.findByText("person@example.com");
  expect(screen.queryByText("Legacy name")).toBeNull();
  fireEvent.click(screen.getByText("•••")); fireEvent.click(screen.getByRole("button", { name: "Edit category" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Category (optional)" }), { target: { value: "Research / client blue" } });
  fireEvent.click(screen.getByRole("button", { name: "Save category" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "category", "profile-a", "Research / client blue"));
  await waitFor(() => expect(screen.queryByRole("textbox")).toBeNull());
  fireEvent.click(screen.getByText("•••"));
  fireEvent.click(screen.getByRole("button", { name: "Edit category" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Category (optional)" }), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save category" }));
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "category", "profile-a", ""));
});
it("requires opt-in in Settings before automatically saving accounts", async () => {
  setup(true); const checkbox = await screen.findByRole("checkbox", { name: "Automatically save accounts I sign in to" });
  await waitFor(() => expect(checkbox).toBeEnabled()); expect(checkbox).not.toBeChecked();
  expect(api.accountAction).not.toHaveBeenCalled(); fireEvent.click(checkbox);
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "remember"));
});
it("can disable automatic saving even when the profile vault cannot be read", async () => {
  vi.mocked(api.readAccountPreferences).mockResolvedValue(true);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  vi.mocked(api.readAccounts).mockRejectedValue(new Error("Vault unavailable"));
  render(<QueryClientProvider client={client}><AccountPreferences /></QueryClientProvider>);
  const checkbox = await screen.findByRole("checkbox"); await waitFor(() => expect(checkbox).toBeChecked());
  fireEvent.click(checkbox);
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "stopRemembering"));
});
it("checks billing for the card's saved identity without activating it", async () => {
  setup(); vi.mocked(api.connectCodexBilling).mockResolvedValue();
  const card = await screen.findByRole("region", { name: "person@example.com" });
  fireEvent.click(await within(card).findByRole("button", { name: "Connect billing" }));
  await waitFor(() => expect(api.connectCodexBilling).toHaveBeenCalledWith("profile:billing-a"));
  expect(api.accountAction).not.toHaveBeenCalled();
});

it("keeps cancellation on the initiating card and permits retry after cleanup", async () => {
  let finish!: () => void;
  vi.mocked(api.addAccount).mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
  vi.mocked(api.cancelAccountLogin).mockResolvedValue();
  setup();
  const card = await screen.findByRole("region", {name: "person@example.com"});
  fireEvent.click(within(card).getByLabelText("More actions for person@example.com"));
  fireEvent.click(within(card).getByRole("button", {name: "Sign in again"}));
  const cancel = within(card).getByRole("button", {name: "Cancel sign-in"});
  expect(cancel).toBeEnabled();
  const operation = vi.mocked(api.addAccount).mock.calls.at(-1)![1];
  fireEvent.click(cancel);
  await waitFor(() => expect(api.cancelAccountLogin).toHaveBeenCalledWith(operation));
  expect(within(card).getByRole("button", {name: "Canceling…"})).toBeDisabled();
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

it("does not steal focus when a category save finishes after its editor was dismissed", async () => {
  setup();
  let finish!: () => void;
  vi.mocked(api.accountAction).mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
  await screen.findByText("person@example.com");
  fireEvent.click(screen.getByRole("button", {name: "More actions for person@example.com"}));
  fireEvent.click(screen.getByRole("button", {name: "Edit category"}));
  fireEvent.click(screen.getByRole("button", {name: "Save category"}));
  const trigger = screen.getByRole("button", {name: "More actions for person@example.com"});
  fireEvent.pointerDown(document.body);
  const outside = document.createElement("button");
  document.body.append(outside); outside.focus();
  try {
    await act(async () => finish());
    await waitFor(() => expect(trigger).toHaveAttribute("aria-expanded", "false"));
    expect(outside).toHaveFocus();
  } finally { outside.remove(); }
});

it("retries a denied vault unlock without starting sign-in or changing accounts", async () => {
  const client = setup();
  await screen.findByRole("region", {name: "person@example.com"});
  const restored = vi.mocked(api.readAccounts).getMockImplementation()!;
  const restoredBilling = vi.mocked(api.readCodexSubscription).getMockImplementation()!;
  vi.mocked(api.readAccounts).mockRejectedValue(new Error("Keychain access denied"));
  vi.mocked(api.readCodexSubscription).mockRejectedValue(new Error("Keychain access denied"));
  await act(async () => { await Promise.all([
    client.invalidateQueries({queryKey: ["accounts"]}),
    client.invalidateQueries({queryKey: ["subscription", "codex"]}),
  ]); });
  expect(screen.getByText("Could not read subscription details.")).toBeTruthy();
  const retry = await screen.findByRole("button", {name: "Retry account access"});
  vi.mocked(api.accountAction).mockImplementation(async () => {
    vi.mocked(api.readAccounts).mockImplementation(restored);
    vi.mocked(api.readCodexSubscription).mockImplementation(restoredBilling);
  });
  fireEvent.click(retry);
  await waitFor(() => expect(api.accountAction).toHaveBeenCalledWith("codex", "unlock", undefined, undefined));
  await waitFor(() => expect(screen.queryByRole("button", {name: "Retry account access"})).toBeNull());
  await waitFor(() => expect(screen.queryByText("Could not read subscription details.")).toBeNull());
  expect(screen.getByRole("button", {name: "Use account"})).toBeEnabled();
  expect(api.addAccount).not.toHaveBeenCalled();
});


it("offers a retry after a billing failure and clears the error when it succeeds", async () => {
  setup();
  vi.mocked(api.connectCodexBilling).mockRejectedValueOnce(new Error("Could not read the browser session."));
  fireEvent.click(await screen.findByRole("button", {name:"Connect billing"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not read the browser session.");
  expect(screen.getByRole("button", {name:"Use account"})).toBeEnabled();
  vi.mocked(api.connectCodexBilling).mockResolvedValueOnce();
  fireEvent.click(screen.getByRole("button", {name:"Retry billing"}));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});
