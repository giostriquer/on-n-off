import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { LimitsColumnEntry } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";
import { SharedReads } from "$lib/sharedReads/announcements";
import { Limits } from "./Limits";
import { NOW, okClaude, okCodex, withSavedProfiles } from "./readingFixtures";

const readAccounts = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts,
  addAccount: vi.fn(), accountAction: vi.fn(),
  removeLimitsAccount: vi.fn().mockResolvedValue(undefined), setLimitsAccountArchived: vi.fn().mockResolvedValue(undefined),
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  cancelAccountLogin: vi.fn().mockResolvedValue(undefined), consumeCodexResetCredit: vi.fn(), pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn(),
  onSharedReadChanged: () => Promise.resolve(() => undefined),
  readCodexSubscription: vi.fn().mockResolvedValue(null),
}));

const DAY = 86_400_000;
const TAG = "Claude Desktop was last signed in to this account";

function profile(overrides: Partial<SavedProfile> = {}): SavedProfile {
  return {
    id: "saved-me", observationId: "uuid-1", legacyId: "me", identity: { provider: "claude", userId: "me", workspaceId: "team" },
    label: "me@claude.example", email: "me@claude.example", savedAt: NOW, active: true, loginState: { state: "usable" }, ...overrides,
  };
}

function answer(claude: LimitsColumnEntry[], profiles: SavedProfile[] = []) {
  readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? withSavedProfiles(claude, profiles) : agentId === "codex" ? [okCodex()] : []));
  readAccounts.mockImplementation((provider: AgentId) => Promise.resolve({
    profiles: provider === "claude" ? profiles : [], nativeAccount: null, recoveryRequired: false, notice: null,
  }));
}

function renderLimits() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  render(<QueryClientProvider client={client}><SharedReads /><Limits resetAlerts={{}} onResetAlertsChange={async () => undefined} /></QueryClientProvider>);
}

const signedInCard = () => screen.findByRole("region", { name: "Claude limits · me@claude.example" });
const desktopCard = () => screen.findByRole("region", { name: "Claude limits · desk@claude.example" });

function desk(overrides: Partial<LimitsColumnEntry> = {}): LimitsColumnEntry {
  return okClaude({ account: { id: "uuid-2", label: "desk@claude.example" }, currentAccount: false, savedProfile: true, ...overrides });
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readAccounts.mockReset();
  readLimits.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Claude Desktop on another account", () => {
  it("is a Desktop tag in the header of the card Claude Desktop is on", async () => {
    answer([okClaude(), desk({ desktopAccount: true })]);
    renderLimits();
    const card = await desktopCard();
    const tag = await within(card).findByRole("button", { name: TAG });
    expect(tag.closest("header")).toBeTruthy();
    expect(tag).toHaveClass("term-badge--neutral");
    expect(tag).toHaveTextContent(/^Desktop$/);
    fireEvent.focus(tag);
    expect(screen.getByRole("tooltip")).toHaveTextContent(/^Claude Desktop was last signed in to this account\.$/);
  });

  it("leaves the signed-in card untagged", async () => {
    answer([okClaude(), desk({ desktopAccount: true })]);
    renderLimits();
    await within(await desktopCard()).findByRole("button", { name: TAG });
    const card = await signedInCard();
    expect(within(card).queryByRole("button", { name: /Desktop/ })).toBeNull();
    expect(within(card).queryByText(/Desktop/)).toBeNull();
  });

  it("is not shown while no card carries Desktop's account", async () => {
    answer([okClaude(), desk()]);
    renderLimits();
    const card = await desktopCard();
    await within(card).findByText("Max");
    expect(screen.queryByRole("button", { name: TAG })).toBeNull();
    expect(screen.queryByText("Desktop")).toBeNull();
  });

  it("sits after the sign-in tag and before the plan", async () => {
    const endsAt = new Date(Date.parse(NOW) + 3 * DAY).toISOString();
    const saved = profile({
      id: "saved-desk", observationId: "uuid-2", legacyId: "desk", identity: { provider: "claude", userId: "desk", workspaceId: "team" },
      label: "desk@claude.example", email: "desk@claude.example", active: false, loginState: { state: "ending", endsAt },
    });
    answer([okClaude(), desk({ desktopAccount: true })], [saved]);
    renderLimits();
    const card = await desktopCard();
    await within(card).findByRole("button", { name: TAG });
    const tags = within(card).getByTestId("account-tags");
    expect(Array.from(tags.children, tag => tag.textContent)).toEqual(["Sign-in ends Aug 20", "Desktop", "Max"]);
  });
});
