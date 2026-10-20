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
const TAG = "Claude Desktop is signed in to another account";

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
  it("is a warning tag in the signed-in card's header naming Desktop's account", async () => {
    answer([okClaude({ otherDesktopAccount: { label: "desk@claude.example" } })]);
    renderLimits();
    const card = await signedInCard();
    const tag = await within(card).findByRole("button", { name: TAG });
    expect(tag.closest("header")).toBeTruthy();
    expect(tag).toHaveClass("term-badge--warning");
    expect(tag).toHaveTextContent("Desktop: other account");
    expect(parseFloat(tag.querySelector<HTMLElement>(".term-badge-fill")!.style.width)).toBe(0);
    fireEvent.focus(tag);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Claude Desktop is signed in to desk@claude.example.");
    expect(screen.getByRole("tooltip")).toHaveTextContent("Sign in to this account in Claude Desktop to use it there.");
  });

  it("says another account when Desktop's account is not one Limits shows", async () => {
    answer([okClaude({ otherDesktopAccount: { label: null } })]);
    renderLimits();
    const tag = await within(await signedInCard()).findByRole("button", { name: TAG });
    fireEvent.focus(tag);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Claude Desktop is signed in to another account.");
  });

  it("is not shown while the read carries no other Desktop account", async () => {
    answer([okClaude()]);
    renderLimits();
    const card = await signedInCard();
    await within(card).findByText("Max");
    expect(within(card).queryByRole("button", { name: TAG })).toBeNull();
    expect(within(card).queryByText("Desktop: other account")).toBeNull();
  });

  it("sits after the sign-in tag and before the plan", async () => {
    const endsAt = new Date(Date.parse(NOW) + 3 * DAY).toISOString();
    answer([okClaude({ otherDesktopAccount: { label: "desk@claude.example" } })], [profile({ loginState: { state: "ending", endsAt } })]);
    renderLimits();
    const card = await signedInCard();
    await within(card).findByRole("button", { name: TAG });
    const tags = within(card).getByTestId("account-tags");
    expect(Array.from(tags.children, tag => tag.textContent)).toEqual(["Sign-in ends Aug 20", "Desktop: other account", "Max"]);
  });
});
