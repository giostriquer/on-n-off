import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import { hasSavedAccounts, LIMITS_PROVIDERS, providerLabel, type LimitsProvider } from "$lib/providers";
import type { AgentId } from "$lib/types";
import { Limits } from "./Limits";
import { NOW, okClaude, okCodex, okMuse, staleCodex } from "./readingFixtures";

const readAccounts = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const forgetLimitsSnapshot = vi.hoisted(() => vi.fn());
const setLimitsArchived = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, accountAction: vi.fn(), forgetLimitsSnapshot, setLimitsArchived,
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  addAccount: vi.fn(), cancelAccountLogin: vi.fn(), consumeCodexResetCredit: vi.fn(), pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn(),
  onSharedReadChanged: () => Promise.resolve(() => {}), readCodexSubscription: vi.fn().mockResolvedValue(null),
}));

const CURRENT: Record<LimitsProvider, (overrides?: Partial<ProviderLimits>) => ProviderLimits> = { claude: okClaude, codex: okCodex, muse: okMuse };

const label = (provider: LimitsProvider) => CURRENT[provider]().account!.label!;
const oldLabel = (provider: LimitsProvider) => `old@${provider}.example`;

function remembered(provider: LimitsProvider): ProviderLimits {
  return CURRENT[provider]({ currentAccount: false, account: { id: `${provider}-old`, label: oldLabel(provider) } });
}

function answer(read: (provider: LimitsProvider) => ProviderLimits[] | Promise<ProviderLimits[]>) {
  readLimits.mockImplementation((provider: LimitsProvider) => Promise.resolve(read(provider)));
}

function renderLimits() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  return render(<QueryClientProvider client={client}><Limits resetAlerts={{}} onResetAlertsChange={async () => undefined} /></QueryClientProvider>);
}

function card(name: string) {
  return screen.getByRole("region", { name });
}

function openMenu(region: HTMLElement, name: string) {
  fireEvent.click(within(region).getByRole("button", { name: `More actions for ${name}` }));
  return within(screen.getByRole("group", { name: `Actions for ${name}` })).getAllByRole("button").map(item => item.textContent);
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  readAccounts.mockReset().mockResolvedValue({ profiles: [], nativeAccount: null, recoveryRequired: false, notice: null });
  readLimits.mockReset();
  forgetLimitsSnapshot.mockReset().mockResolvedValue(undefined);
  setLimitsArchived.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe.each(LIMITS_PROVIDERS)("every %s card", (provider) => {
  const name = providerLabel(provider);

  it("has a header with its tags and a menu that checks limits now", async () => {
    answer(read => [CURRENT[read](), remembered(read)]);
    renderLimits();
    const current = await screen.findByRole("region", { name: `${name} limits · ${label(provider)}` });
    expect(within(current).getByTestId("account-tags")).toBeInTheDocument();
    const items = openMenu(current, label(provider));
    expect(items[0]).toBe("Check limits now");
    if (hasSavedAccounts(provider)) expect(items).toContain("Sign out");
    else expect(items).toEqual(["Check limits now"]);
  });

  it("archives a remembered account from its menu", async () => {
    answer(read => [CURRENT[read](), remembered(read)]);
    renderLimits();
    const old = await screen.findByRole("region", { name: `${name} limits · ${oldLabel(provider)}` });
    expect(openMenu(old, oldLabel(provider))).toEqual(expect.arrayContaining(["Check limits now", "Archive account", "Remove account"]));
    fireEvent.click(screen.getByRole("button", { name: "Archive account" }));
    await waitFor(() => expect(setLimitsArchived).toHaveBeenCalledWith(provider, [`${provider}-old`], true));
    await waitFor(() => expect(screen.queryByRole("region", { name: `${name} limits · ${oldLabel(provider)}` })).toBeNull());
  });

  it("removes a remembered account's history only once its removal is confirmed", async () => {
    answer(read => [CURRENT[read](), remembered(read)]);
    renderLimits();
    const old = await screen.findByRole("region", { name: `${name} limits · ${oldLabel(provider)}` });
    openMenu(old, oldLabel(provider));
    fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
    expect(forgetLimitsSnapshot).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(forgetLimitsSnapshot).toHaveBeenCalledWith(provider, `${provider}-old`, undefined));
    await waitFor(() => expect(screen.queryByRole("region", { name: `${name} limits · ${oldLabel(provider)}` })).toBeNull());
  });

  it("offers Check limits now while its first read is still pending", async () => {
    answer(read => read === provider ? new Promise<ProviderLimits[]>(() => {}) : [CURRENT[read]()]);
    renderLimits();
    const pending = await screen.findByRole("region", { name: `${name} limits` });
    expect(pending.getAttribute("data-status")).toBe("pending");
    expect(openMenu(pending, name)).toEqual(["Check limits now"]);
  });

  it("checks only its own provider again, with force, from Check limits now", async () => {
    answer(read => [CURRENT[read]()]);
    renderLimits();
    const current = await screen.findByRole("region", { name: `${name} limits · ${label(provider)}` });
    await waitFor(() => expect(readLimits).toHaveBeenCalledTimes(LIMITS_PROVIDERS.length));
    openMenu(current, label(provider));
    fireEvent.click(screen.getByRole("button", { name: "Check limits now" }));
    await waitFor(() => expect(readLimits).toHaveBeenCalledWith(provider, true));
    expect(readLimits.mock.calls.filter(([, force]) => force)).toEqual([[provider, true]]);
    expect(screen.queryByRole("group", { name: `Actions for ${label(provider)}` })).toBeNull();
  });
});

describe("card header", () => {
  function answerClaudeCodex(claude: ProviderLimits[], codex: ProviderLimits[]) {
    readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? claude : agentId === "codex" ? codex : []));
  }

  async function claudeTags(widths: { line: number; name: number }) {
    vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(widths.line);
    vi.spyOn(Element.prototype, "scrollWidth", "get").mockReturnValue(widths.name);
    answerClaudeCodex([okClaude({ plan: "max ×5" })], [okCodex()]);
    const { unmount } = renderLimits();
    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const tags = within(claude).getByTestId("account-tags");
    expect(within(tags).getByText("Max ×5")).toBeTruthy();
    return { claude, tags, line: within(claude).getByTitle("me@claude.example").parentElement!, unmount };
  }

  it("lays an account's tags on its name's line when they fit there", async () => {
    try {
      const { tags, line } = await claudeTags({ line: 300, name: 120 });
      expect(tags.parentElement).toBe(line);
    } finally {
      vi.restoreAllMocks();
    }
  });

  it("moves an account's tags to their own row when its name is squeezed without a re-render", async () => {
    const watching = new Map<Element, () => void>();
    vi.stubGlobal("ResizeObserver", class {
      constructor(private readonly changed: () => void) {}
      observe(node: Element) { watching.set(node, this.changed); }
      disconnect() { watching.clear(); }
    });
    try {
      const { claude, tags, line, unmount } = await claudeTags({ line: 300, name: 120 });
      expect(tags.parentElement).toBe(line);
      vi.spyOn(Element.prototype, "scrollWidth", "get").mockReturnValue(400);
      act(() => watching.get(within(claude).getByTitle("me@claude.example"))?.());
      expect(within(claude).getByTestId("account-tags").parentElement).toBe(claude.querySelector("header"));
      unmount();
      expect(watching.size, "every observer lets go of its nodes on unmount").toBe(0);
    } finally {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  it("gives an account's tags their own row across the header when they do not fit beside its name", async () => {
    try {
      const { claude, tags, line } = await claudeTags({ line: 300, name: 400 });
      expect(line.contains(tags)).toBe(false);
      expect(tags.parentElement).toBe(claude.querySelector("header"));
    } finally {
      vi.restoreAllMocks();
    }
  });

  it("draws the active account's dot beside its headline window, or in its header without one", async () => {
    const session = { id: "secondary", label: "5 hour · all models", kind: "session" as const, usedPercent: 12, resetsAt: "2026-08-17T23:00:00Z", observedAt: NOW };
    answerClaudeCodex([okClaude({ plan: "max ×5" })], [okCodex({ windows: [session] }), staleCodex()]);
    renderLimits();
    const claude = await screen.findByRole("region", { name: "Claude limits · me@claude.example" });
    const beside = within(claude).getByRole("img", { name: "Active account" });
    expect(within(beside.parentElement!).getByText("Weekly · all models")).toBeTruthy();
    expect(claude.querySelector("header")!.contains(beside)).toBe(false);
    expect(within(claude).queryByText("ACTIVE")).toBeNull();
    expect(within(claude).getByText("Max ×5")).toBeTruthy();
    const codex = card("Codex limits · work@codex.example");
    const dots = within(codex).getAllByRole("img", { name: "Active account" });
    expect(dots).toHaveLength(1);
    expect(codex.querySelector("header")!.contains(dots[0])).toBe(true);
    expect(within(codex).getByRole("meter", { name: "5 hour · all models" })).toBeTruthy();
    expect(within(card("Codex limits · personal@codex.example")).queryByRole("img", { name: "Active account" })).toBeNull();
  });
});
