import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AccountsReading, SavedProfile } from "$lib/accountTypes";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import type { LimitsColumnEntry } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { AppSettingsProvider } from "$lib/useAppSettings";
import { Limits } from "./Limits";
import { LimitsPopover } from "./LimitsPopover";
import { NOW, okClaude, okCodex, staleCodex } from "./readingFixtures";

const readLimits = vi.hoisted(() => vi.fn());
const readAccounts = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts,
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  accountAction: vi.fn(), addAccount: vi.fn(), cancelAccountLogin: vi.fn(), removeLimitsAccount: vi.fn(), setLimitsAccountArchived: vi.fn(),
  setAccountCategory: vi.fn(), readCodexSubscription: vi.fn().mockResolvedValue(null),
  consumeCodexResetCredit: vi.fn(), pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn(),
  onLimitsPopoverOpened: vi.fn().mockResolvedValue(() => undefined), hideLimitsPopover: vi.fn(), openLimitsWindow: vi.fn(), quitApp: vi.fn(),
  loadAppSettings: vi.fn().mockResolvedValue(DEFAULT_APP_SETTINGS), onSharedReadChanged: () => Promise.resolve(() => undefined),
}));

const ENDED = "2026-08-16T20:00:00Z";

function profile(provider: AgentId, id: string, email: string, overrides: Partial<SavedProfile> = {}): SavedProfile {
  return {
    id, observationId: `profile:${id}`, legacyId: id, identity: { provider, userId: id, workspaceId: "team" },
    label: email, email, savedAt: NOW, active: false, loginState: { state: "usable" as const }, ...overrides,
  };
}

const claudeMe = profile("claude", "me", "me@claude.example", { active: true, loginState: { state: "ended", endsAt: ENDED } });
const codexWork = profile("codex", "work", "work@codex.example", { active: true });
const codexShared = profile("codex", "shared", "shared@codex.example");
const codexFresh = profile("codex", "fresh", "fresh@codex.example");

const COLUMNS: Record<"claude" | "codex", LimitsColumnEntry[]> = {
  claude: [okClaude({ account: { id: claudeMe.observationId, label: "me@claude.example" }, profile: claudeMe })],
  codex: [
    okCodex({ account: { id: codexWork.observationId, label: "work@codex.example" }, profile: codexWork, category: "Client A" }),
    staleCodex({
      account: { id: codexShared.observationId, label: "shared@codex.example", legacyId: "team" }, savedProfile: true, profile: codexShared,
      replaced: [{ id: "team", label: "shared@codex.example" }],
    }),
    {
      provider: "codex", status: "ok", account: { id: codexFresh.observationId, label: "fresh@codex.example", legacyId: "fresh" },
      currentAccount: false, windows: [], unread: true, profile: codexFresh,
    },
  ],
};

function accountList(profiles: SavedProfile[]): AccountsReading {
  const active = profiles.find(saved => saved.active);
  return { profiles, nativeAccount: active?.identity ?? null, nativeObservationId: active?.observationId ?? null, recoveryRequired: false, notice: null };
}

function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readLimits.mockReset().mockImplementation((provider: AgentId) =>
    Promise.resolve(provider === "claude" || provider === "codex" ? COLUMNS[provider] : []));
  readAccounts.mockReset().mockImplementation((provider: AgentId) =>
    Promise.resolve(accountList(provider === "claude" ? [claudeMe] : provider === "codex" ? [codexWork, codexShared, codexFresh] : [])));
});

afterEach(() => {
  vi.useRealTimers();
});

async function screenCards(provider: string) {
  const view = render(
    <QueryClientProvider client={client()}>
      <Limits pollMinutes={5} resetAlerts={{}} onResetAlertsChange={async () => undefined} />
    </QueryClientProvider>,
  );
  await screen.findByRole("region", { name: "Codex limits · work@codex.example" });
  await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
  const cards = screen.getAllByRole("region", { name: new RegExp(`^${provider} limits`) })
    .map(card => ({ name: card.getAttribute("aria-label"), text: card.textContent ?? "" }));
  view.unmount();
  return cards;
}

async function popoverCards(provider: string) {
  const view = render(
    <QueryClientProvider client={client()}>
      <AppSettingsProvider><LimitsPopover /></AppSettingsProvider>
    </QueryClientProvider>,
  );
  await screen.findByRole("article", { name: "Codex limits · work@codex.example" });
  await screen.findByRole("article", { name: "Claude limits · me@claude.example" });
  const section = screen.getByRole("region", { name: `${provider} accounts` });
  const cards = within(section).getAllByRole("article")
    .map(card => ({ name: card.getAttribute("aria-label"), text: card.textContent ?? "" }));
  const count = within(section).getByText(/^\d+ accounts?$/).textContent;
  view.unmount();
  return { cards, count };
}

it.each(["Claude", "Codex"])("draws the same %s accounts on the screen and in the popover from one Limits read", async (provider) => {
  const onScreen = await screenCards(provider);
  const inPopover = await popoverCards(provider);

  expect(inPopover.cards.map(card => card.name)).toEqual(onScreen.map(card => card.name));
  expect(inPopover.count).toBe(`${onScreen.length} ${onScreen.length === 1 ? "account" : "accounts"}`);
  for (const [index, card] of inPopover.cards.entries()) {
    for (const copy of ["Usage unavailable.", "reported no rate-limit windows.", "Claude ended this sign-in on Aug 16."]) {
      expect(card.text.includes(copy), `${card.name}: ${copy}`).toBe(onScreen[index].text.includes(copy));
    }
  }
});

it("lists a saved profile no read answered for, and words its empty card as the screen does", async () => {
  const { cards } = await popoverCards("Codex");

  expect(cards.map(card => card.name)).toEqual([
    "Codex limits · work@codex.example", "Codex limits · shared@codex.example", "Codex limits · fresh@codex.example",
  ]);
  expect(cards[2].text).toContain("Usage unavailable.");
});

it("says on the popover that a Claude sign-in has ended", async () => {
  const { cards } = await popoverCards("Claude");

  expect(cards[0].text).toContain("Claude ended this sign-in on Aug 16. Sign in again to use this account.");
});
