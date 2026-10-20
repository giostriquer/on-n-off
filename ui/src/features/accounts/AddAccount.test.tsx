import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as api from "$lib/api";
import type { AccountsReading } from "$lib/accountTypes";
import type { AccountProvider } from "$lib/providers";
import { AccountControllers } from "./AccountManager";
import { AddAccount } from "./AddAccount";

vi.mock("$lib/api", () => ({ readAccounts: vi.fn(), readAccountPreferences: vi.fn(), accountAction: vi.fn(), addAccount: vi.fn(), cancelAccountLogin: vi.fn() }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

const EMPTY: AccountsReading = { profiles: [], nativeAccount: null, recoveryRequired: false, notice: null };

function renderPicker(lists: Record<AccountProvider, () => Promise<AccountsReading>>) {
  vi.mocked(api.readAccountPreferences).mockResolvedValue(false);
  vi.mocked(api.readAccounts).mockImplementation(provider => lists[provider]());
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><AccountControllers><AddAccount /></AccountControllers></QueryClientProvider>);
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  return { client, picker: screen.getByRole("group", { name: "Add account" }) };
}

it("holds a provider's sign-in while its account list cannot act, as the card actions do", async () => {
  const { client, picker } = renderPicker({
    claude: () => Promise.reject(new Error("Keychain access denied")),
    codex: () => Promise.resolve(EMPTY),
    muse: () => Promise.resolve({ ...EMPTY, recoveryRequired: true }),
  });
  const choice = (name: string) => within(picker).getByRole("button", { name });
  await waitFor(() => expect(choice("Codex")).toBeEnabled());
  await waitFor(() => expect(client.getQueryState(["accounts", "claude"])?.status).toBe("error"));
  await waitFor(() => expect(client.getQueryState(["accounts", "muse"])?.status).toBe("success"));
  expect(choice("Claude")).toBeDisabled();
  expect(choice("Muse")).toBeDisabled();
  fireEvent.click(choice("Claude"));
  expect(api.addAccount).not.toHaveBeenCalled();
});

it("holds every provider's sign-in until its account list has been read", () => {
  const { picker } = renderPicker({ claude: () => new Promise(() => {}), codex: () => new Promise(() => {}), muse: () => new Promise(() => {}) });
  expect(within(picker).getAllByRole("button", { name: /^(Claude|Codex|Muse)$/ }).map(button => button.hasAttribute("disabled"))).toEqual([true, true, true]);
});

it("holds a provider's sign-in while one it started runs, and only that provider's", async () => {
  vi.mocked(api.addAccount).mockImplementation(() => new Promise(() => {}));
  vi.mocked(api.cancelAccountLogin).mockResolvedValue();
  const { picker } = renderPicker({ claude: () => Promise.resolve(EMPTY), codex: () => Promise.resolve(EMPTY), muse: () => Promise.resolve(EMPTY) });
  await waitFor(() => expect(within(picker).getByRole("button", { name: "Claude" })).toBeEnabled());
  fireEvent.click(within(picker).getByRole("button", { name: "Claude" }));
  await waitFor(() => expect(api.addAccount).toHaveBeenCalledWith("claude", expect.any(String), undefined));
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  const reopened = screen.getByRole("group", { name: "Add account" });
  expect(within(reopened).getByRole("button", { name: "Claude" })).toBeDisabled();
  expect(within(reopened).getByRole("button", { name: "Codex" })).toBeEnabled();
  expect(within(reopened).getByRole("button", { name: "Muse" })).toBeEnabled();
});
