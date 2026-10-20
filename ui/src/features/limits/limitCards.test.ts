import { describe, expect, it } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import { formatObservedAt } from "$lib/limitsFormat";
import type { LimitsCredits, LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import { LIMITS_PROVIDERS, type LimitsProvider } from "$lib/providers";
import type { AgentId } from "$lib/types";
import { cardTags } from "./cardTags";
import { limitCards, limitColumn, type LimitCard } from "./limitCards";
import { NOW, lastObserved, okClaude, okCodex, staleCodex, statusOnly } from "./readingFixtures";

const AT_NOW = Date.parse(NOW);

function saved(provider: AgentId, id: string, observationId: string, email: string, overrides: Partial<SavedProfile> = {}): SavedProfile {
  return { id, observationId, legacyId: id, identity: { provider, userId: id, workspaceId: id }, email, label: email, savedAt: NOW, active: false, needsLogin: false, ...overrides };
}

function cards(entries: ProviderLimits[] | undefined, profiles: SavedProfile[] = [], now = AT_NOW, provider = (entries?.[0]?.provider ?? "codex") as LimitsProvider): LimitCard[] {
  const shown = limitCards({ provider, entries, profiles, nativeLogin: null, now });
  if (!shown) throw new Error("expected cards");
  return shown;
}

const labels = (shown: LimitCard[]) => shown.map(card => card.identity.label);

describe("which cards a provider shows", () => {
  it("is nothing until a read answers or a profile is saved, and no card once a read answers with none", () => {
    expect(limitCards({ provider: "codex", entries: undefined, profiles: [], nativeLogin: null, now: AT_NOW })).toBeNull();
    expect(limitCards({ provider: "codex", entries: [], profiles: [], nativeLogin: null, now: AT_NOW })).toEqual([]);
  });

  it("gives a saved profile no read has answered for a card of its own, even before any read", () => {
    const profile = saved("claude", "unread", "profile:unread", "unread@claude.example", { active: true });
    const [card] = cards(undefined, [profile], AT_NOW, "claude");
    expect(card).toEqual({
      key: "current-profile:unread", provider: "claude",
      identity: { label: "unread@claude.example", ariaLabel: "Claude limits · unread@claude.example", category: null },
      account: { id: "profile:unread", name: "unread@claude.example", profile, forget: [{ accountId: "profile:unread" }] },
      reading: null, plan: null, status: null, signedIn: "confirmed", leads: true, headline: null, rows: [],
      figures: { ownBalance: null, workspaceShare: null, creditsSpent: null, bankedResets: null, paidOffer: null },
      empty: { reason: "usageUnavailable", copy: "Usage unavailable." },
      freshness: { updatedAt: null, message: null, readStatus: "ok" },
      subscription: null,
      signIn: null,
      archived: false,
    });
  });

  it("gives a saved Codex profile no read answered for its own subscription read, and no reset to spend", () => {
    const [card] = cards(undefined, [saved("codex", "unread", "profile:unread", "unread@codex.example")], AT_NOW, "codex");
    expect(card).toMatchObject({
      provider: "codex", signedIn: "no", key: "remembered-profile:unread", account: { id: "profile:unread" }, reading: null,
      subscription: { accountId: "profile:unread", poll: false, term: null },
      empty: { reason: "usageUnavailable", copy: "Usage unavailable." },
    });
  });

  it("keeps an unverified legacy card beside its saved login", () => {
    const shown = cards([okCodex({ account: { id: "team", label: "shared@example.com" }, currentAccount: false })],
      [saved("codex", "saved", "profile:user-team", "shared@example.com", { legacyId: "team", identity: { provider: "codex", userId: "user", workspaceId: "team" } })]);
    expect(labels(shown)).toEqual(["shared@example.com", "shared@example.com"]);
    expect(shown.map(card => [card.account?.id, card.empty?.reason ?? null])).toEqual([["team", null], ["profile:user-team", "usageUnavailable"]]);
  });

  it("reconciles legacy cards from a saved identity when an older app drops the snapshot alias, and forgets the legacy history first", () => {
    const profile = saved("codex", "saved", "profile:user-team", "shared@example.com", { legacyId: "team", identity: { provider: "codex", userId: "user", workspaceId: "team" } });
    const scoped = okCodex({ account: { id: "profile:user-team", label: "shared@example.com" }, currentAccount: false,
      windows: [{ id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 0, observedAt: NOW }] });
    const legacy = okCodex({ account: { id: "team", label: "shared@example.com" }, currentAccount: false });
    const shown = cards([scoped, legacy, staleCodex()], [profile]);
    expect(labels(shown)).toEqual(["shared@example.com", "personal@codex.example"]);
    expect(shown[0].headline?.text).toBe("0%");
    expect(shown[0].account?.forget).toEqual([{ accountId: "team", expectedEmail: "shared@example.com" }, { accountId: "profile:user-team" }]);
    expect(shown[1].account?.forget).toEqual([{ accountId: "acct-personal" }]);
  });
});

describe("card order", () => {
  const ORDER_NOW = Date.parse("2026-09-21T12:00:00Z");
  const at = (hours: number) => new Date(ORDER_NOW + hours * 3_600_000).toISOString();
  let sequence = 0;
  function account(
    label: string,
    windows: { session?: [number, string | null]; weekly?: [number, string | null]; model?: [number, string | null] },
    overrides: Partial<ProviderLimits> = {},
  ): ProviderLimits {
    const window = (kind: "session" | "weekly" | "model", [usedPercent, resetsAt]: [number, string | null]) =>
      ({ id: kind, label: kind, kind, usedPercent, resetsAt, observedAt: "2026-09-21T11:00:00Z" });
    return {
      provider: "claude", status: "ok", account: { id: `id-${sequence++}`, label }, currentAccount: false, plan: "max ×5", planLabel: "Max ×5", planCapacity: 5,
      windows: [
        ...(windows.session ? [window("session", windows.session)] : []),
        ...(windows.weekly ? [window("weekly", windows.weekly)] : []),
        ...(windows.model ? [window("model", windows.model)] : []),
      ],
      ...overrides,
    };
  }
  const order = (entries: ProviderLimits[], profiles: SavedProfile[] = [], provider: LimitsProvider = "claude") => labels(cards(entries, profiles, ORDER_NOW, provider));

  it("puts the active account first even when it is out of usage", () => {
    const active = account("active", { session: [100, at(2)], weekly: [100, at(48)] }, { currentAccount: true });
    const fresh = account("fresh", { session: [0, at(5)], weekly: [0, at(100)] });
    expect(order([fresh, active])).toEqual(["active", "fresh"]);
  });

  it("ranks usage left above waiting for a reset, however soon that reset is", () => {
    const nearlyOut = account("nearly out", { session: [99, at(4)], weekly: [30, at(100)] });
    const resetsInMinutes = account("resets in minutes", { session: [100, at(0.1)], weekly: [10, at(100)] });
    expect(order([resetsInMinutes, nearlyOut])).toEqual(["nearly out", "resets in minutes"]);
  });

  it("weighs usage left by the plan's capacity the reading reports, so a bigger plan with less left can still win", () => {
    const sized = (label: string, used: number, planCapacity: number | null) =>
      account(label, { session: [used, at(3)], weekly: [used, at(100)] }, { plan: "any", planLabel: "Any", planCapacity });
    const big = sized("×20 at 30% left", 70, 20);
    const small = sized("×5 untouched", 0, 5);
    expect(order([small, big])).toEqual(["×20 at 30% left", "×5 untouched"]);
    const bigLow = sized("×20 at 20% left", 80, 20);
    expect(order([bigLow, small])).toEqual(["×5 untouched", "×20 at 20% left"]);
    const unsized = sized("no capacity untouched", 0, null);
    const fiveAtThirty = sized("×5 at 30% left", 70, 5);
    expect(order([unsized, fiveAtThirty])).toEqual(["×5 at 30% left", "no capacity untouched"]);
    const fiveAtTen = sized("×5 at 10% left", 90, 5);
    expect(order([fiveAtTen, unsized])).toEqual(["no capacity untouched", "×5 at 10% left"]);
    const textOnly = account("a label that reads ×20", { weekly: [70, at(100)] }, { plan: "max ×20", planLabel: "Max ×20", planCapacity: undefined });
    const plain = account("plain untouched", { weekly: [0, at(100)] }, { plan: "pro", planLabel: "Pro", planCapacity: null });
    expect(order([textOnly, plain])).toEqual(["plain untouched", "a label that reads ×20"]);
  });

  it("shows the plan's label as the reading reports it, and none without one", () => {
    for (const planLabel of ["Max ×20", "Pro ×5", "Self serve business prolite", "Muse Code Power Usage"]) {
      expect(cards([okCodex({ plan: "raw_id", planLabel })])[0].plan).toBe(planLabel);
    }
    expect(cards([okCodex({ plan: "pro", planLabel: null })])[0].plan).toBeNull();
    expect(cards([okCodex({ plan: "pro", planLabel: undefined })])[0].plan).toBeNull();
  });

  it("measures usage left by the fuller of the two main windows, ignoring model buckets", () => {
    const sessionBound = account("session nearly full", { session: [90, at(1)], weekly: [10, at(100)] });
    const balanced = account("balanced", { session: [50, at(1)], weekly: [50, at(100)] });
    const opusFull = account("opus full", { session: [20, at(1)], weekly: [20, at(100)], model: [100, at(100)] });
    expect(order([sessionBound, balanced, opusFull])).toEqual(["opus full", "balanced", "session nearly full"]);
  });

  it("ignores a full model bucket when working out when an exhausted account is usable again", () => {
    const opusToo = account("opus bucket full too", { session: [100, at(1)], weekly: [30, at(100)], model: [100, at(96)] });
    const laterAnyway = account("weekly full for two days", { session: [10, at(1)], weekly: [100, at(48)] });
    expect(order([laterAnyway, opusToo])).toEqual(["opus bucket full too", "weekly full for two days"]);
  });

  it("orders accounts that are out of usage by when they become usable again: the last full window's reset", () => {
    const weeklyFull = account("weekly full for two days", { session: [100, at(1)], weekly: [100, at(48)] });
    const sessionFull = account("session full for three hours", { session: [100, at(3)], weekly: [40, at(100)] });
    const noReset = account("no reset reported", { session: [100, null], weekly: [20, at(100)] });
    expect(order([noReset, weeklyFull, sessionFull])).toEqual(["session full for three hours", "weekly full for two days", "no reset reported"]);
  });

  it("treats a full window whose reset has passed as renewed, so the account has usage again", () => {
    const renewed = account("renewed", { session: [100, at(-1)], weekly: [30, at(100)] });
    const waiting = account("waiting", { session: [60, at(1)], weekly: [60, at(100)] });
    expect(order([waiting, renewed])).toEqual(["renewed", "waiting"]);
  });

  it("puts accounts with unknown usage last and keeps the incoming order between equals", () => {
    const unknownA = account("unknown a", {});
    const unknownB = account("unknown b", {});
    const twinA = account("twin a", { session: [40, at(50)], weekly: [40, at(100)] });
    const twinB = account("twin b", { session: [40, at(2)], weekly: [40, at(100)] });
    const out = account("out", { session: [100, at(2)], weekly: [0, at(100)] });
    expect(order([unknownA, twinA, out, unknownB, twinB])).toEqual(["twin a", "twin b", "out", "unknown a", "unknown b"]);
    const neverA = account("never a", { session: [100, null] });
    const neverB = account("never b", { session: [100, null] });
    expect(order([neverB, neverA, out])).toEqual(["out", "never b", "never a"]);
  });

  it("orders a saved profile no read answered like the rest: first as the active login, with the unknown ones otherwise", () => {
    const current = account("current", { weekly: [90, at(100)] }, { currentAccount: true });
    const spare = account("spare", { weekly: [30, at(100)] });
    const unknown = account("unknown", {});
    const profile = (id: string, active: boolean) => saved("claude", id, `profile:${id}`, `${id}@example.com`, { active });
    expect(order([unknown, spare, current], [profile("idle", false), profile("ghost", true)]))
      .toEqual(["current", "ghost@example.com", "spare", "unknown", "idle@example.com"]);
  });
});

describe("a card's identity", () => {
  it("is named by its saved profile's email and its account's category, and is active as the profiles say", () => {
    const shown = limitCards({
      provider: "codex",
      entries: [okCodex({ account: { id: "acct-work", label: "old-label@codex.example" } }), staleCodex()],
      profiles: [
        saved("codex", "work", "acct-work", "work@codex.example", { category: "Stale copy" }),
        saved("codex", "personal", "acct-personal", "personal@codex.example", { active: true, category: "Cleared since" }),
      ],
      categories: { "acct-work": "Client A", "acct-other": "Someone else" },
      nativeLogin: null,
      now: AT_NOW,
    })!;
    expect(shown.map(({ identity, account, signedIn }) => ({ ...identity, name: account?.name, signedIn }))).toEqual([
      { label: "work@codex.example", ariaLabel: "Codex limits · work@codex.example", category: "Client A", name: "work@codex.example", signedIn: "no" },
      { label: "personal@codex.example", ariaLabel: "Codex limits · personal@codex.example", category: null, name: "personal@codex.example", signedIn: "confirmed" },
    ]);
  });

  it.each([
    ["the read's label", { account: { id: "acct-work", label: "work@codex.example" } }, { label: "work@codex.example", ariaLabel: "Codex limits · work@codex.example", category: null }, "work@codex.example"],
    ["the account id for the controls when the read has no label", { account: { id: "acct-work", label: null } }, { label: "Codex", ariaLabel: "Codex limits", category: null }, "acct-work"],
    ["the provider, with no account to control, when there is no account", { account: null }, { label: "Codex", ariaLabel: "Codex limits", category: null }, null],
  ] as const)("falls back to %s", (_case, overrides, identity, name) => {
    const [card] = cards([okCodex(overrides)]);
    expect(card.identity).toEqual(identity);
    expect(card.account?.name ?? null).toBe(name);
  });

  it("marks only the accounts in use as signed in, never one without an account, which still leads its column", () => {
    expect(cards([okCodex(), staleCodex()]).map(card => [card.key, card.signedIn])).toEqual([["current-acct-work", "confirmed"], ["remembered-acct-personal", "no"]]);
    expect(cards([staleCodex(), statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.")]).map(card => [card.key, card.signedIn])).toEqual([["current-0", "no"], ["remembered-acct-personal", "no"]]);
  });

  it("tells same-email accounts apart by plan, never by their workspace ids", () => {
    const workspaces = [["personal", "ws-personal-7f3a", "prolite", "Pro ×5"], ["business", "ws-business-9c1e", "business", "Business"]] as const;
    const shown = cards(
      workspaces.map(([id, , plan, planLabel], index) => okCodex({ account: { id: `profile:${id}`, label: "shared@example.com" }, currentAccount: index === 0, plan, planLabel })),
      workspaces.map(([id, workspaceId], index) => saved("codex", id, `profile:${id}`, "shared@example.com", { legacyId: workspaceId, identity: { provider: "codex", userId: "same-user", workspaceId }, active: index === 0 })),
    );
    expect(shown.map(card => [card.identity.label, card.plan])).toEqual([["shared@example.com", "Pro ×5"], ["shared@example.com", "Business"]]);
    for (const card of shown) expect(JSON.stringify([card.identity, card.plan, card.headline, card.rows])).not.toMatch(/ws-personal-7f3a|ws-business-9c1e/);
  });
});

describe("a card's windows", () => {
  it("leads with the weekly window and presents the rest as rows, a passed reset back at zero", () => {
    const [current, remembered] = cards([okCodex(), staleCodex()]);
    expect(current.headline).toMatchObject({ id: "primary", label: "Weekly · all models", usedNow: 74, text: "74%", meter: { level: "blend" } });
    expect(current.rows).toMatchObject([{ id: "extra:gpt-5.6-luna", usedNow: 0, text: "0%", meter: { level: "base" }, note: expect.stringMatching(/^reset 1m ago · \w{3} \d\d:\d\d$/) }]);
    expect(remembered.headline).toMatchObject({ usedNow: 88, text: "88%", meter: { level: "blend" }, note: expect.stringMatching(/^resets in 2d 14h/) });
    expect(remembered.rows).toMatchObject([{ id: "secondary", usedNow: 0, text: "0%", meter: { level: "base" }, note: expect.stringMatching(/^reset 22h ago · \w{3} \d\d:\d\d$/) }]);
  });

  it("presents an elapsed headline window as reset, never as its old 97%", () => {
    const renewed = staleCodex();
    renewed.windows[0] = { ...renewed.windows[0], usedPercent: 97, resetsAt: "2026-08-17T18:35:00Z" };
    expect(cards([renewed])[0].headline).toEqual({
      id: "primary", label: "Weekly · all models", usedNow: 0, usedFigure: 0, meter: { level: "base" }, text: "0%", note: expect.stringMatching(/^reset 1h ago · \w{3} \d\d:\d\d$/),
    });
  });

  it("leads a card with no weekly window with nothing: its session and model windows are ordinary rows", () => {
    const session = { id: "primary", label: "5 hour · all models", kind: "session" as const, usedPercent: 12, resetsAt: "2026-08-17T23:00:00Z", observedAt: NOW };
    const model = { id: "fable", label: "Weekly · Fable", kind: "model" as const, usedPercent: 30, resetsAt: "2026-08-20T23:00:00Z", observedAt: NOW };
    const [card] = cards([okCodex({ windows: [session, model] })]);
    expect(card.headline).toBeNull();
    expect(card.rows.map(row => row.id)).toEqual(["primary", "fable"]);
    expect(card.empty).toBeNull();
  });

  it("gives the headline and every row the note its window reads now, a missing reset included", () => {
    const remembered = okClaude({ currentAccount: false });
    remembered.windows = [
      { ...remembered.windows[0], resetsAt: null },
      { ...remembered.windows[1], usedPercent: 0, resetsAt: null, startsOnFirstMessage: true },
      { ...remembered.windows[2], resetsAt: "invalid" },
    ];
    const [card] = cards([remembered]);
    expect([card.headline, ...card.rows].map(window => [window?.id, window?.note])).toEqual([
      ["weekly_all", "Reset time unavailable"],
      ["session", "Starts with your first message"],
      ["weekly_opus", "Reset time unavailable"],
    ]);
  });

  it.each(LIMITS_PROVIDERS)("gives a %s window its first-message note from the window alone, whatever its kind", (provider) => {
    const waiting = { id: "waiting", label: "Weekly · Model", kind: "model" as const, usedPercent: 0, resetsAt: null, observedAt: NOW, startsOnFirstMessage: true };
    const unflagged = { ...waiting, id: "unflagged", startsOnFirstMessage: false };
    const reading = { ...okCodex({ windows: [waiting, unflagged] }), provider };
    expect(cards([reading], [], AT_NOW, provider)[0].rows.map(row => [row.id, row.note])).toEqual([
      ["waiting", "Starts with your first message"],
      ["unflagged", "Reset time unavailable"],
    ]);
  });

});

describe("a card's status and message", () => {
  it("leaves a saved profile no read answered for without a status or a message", () => {
    const [card] = cards([], [saved("claude", "unread", "profile:unread", "unread@claude.example")], AT_NOW, "claude");
    expect([card.status, card.freshness.message]).toEqual([null, null]);
  });

  it("keeps one source-neutral card for the signed-in Claude account when its refresh is paused", () => {
    const observedAt = "2026-08-16T21:40:00.000Z";
    const message = "Access token expired — send a prompt with `claude` to renew it, then refresh here.";
    const shown = cards([okClaude({
      status: "unauthenticated",
      message,
      standing: lastObserved("signedOut", message),
      windows: [
        { id: "weekly_all", label: "Weekly · all models", kind: "weekly", usedPercent: 12, resetsAt: "2026-08-24T13:59:59Z", observedAt },
        { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 7, resetsAt: "2026-08-17T04:59:59Z", observedAt },
      ],
    })]);
    expect(shown).toHaveLength(1);
    expect(shown[0]).toMatchObject({
      status: { kind: "paused" },
      headline: { usedNow: 12 },
      rows: [{ id: "session", usedNow: 0, note: expect.stringMatching(/^reset 15h ago · \w{3} \d\d:\d\d$/) }],
      freshness: { message: { text: message, tone: "muted" }, updatedAt: formatObservedAt(observedAt), readStatus: "unauthenticated" },
      empty: null,
    });
  });

  it("draws a read that did not answer with its standing's message and no empty copy", () => {
    const text = "Could not reach the Claude usage service (HTTP 503).";
    expect(cards([statusOnly("claude", "failed", text, "error")])[0]).toMatchObject({
      headline: null, rows: [], empty: null, freshness: { message: { text, tone: "error" }, readStatus: "failed" },
    });
  });

  it("keeps remembered accounts, each dated once, when the current login is signed out", () => {
    const [current, remembered] = cards([statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits."), staleCodex()]);
    expect(current).toMatchObject({
      identity: { label: "Codex" },
      freshness: { message: { text: "Sign in with `codex` to see subscription limits.", tone: "muted" }, readStatus: "signedOut", updatedAt: null },
    });
    expect(remembered.freshness.updatedAt).toBe(formatObservedAt("2026-08-16T21:40:00.000Z"));
  });
});

describe("a card's empty copy", () => {
  it("says usage is unavailable for a saved account whose read reported no windows, and names the provider otherwise", () => {
    const empty = okCodex({ account: { id: "profile:empty", label: "empty@codex.example" }, currentAccount: false, windows: [], credits: null });
    const [, withProfile] = cards([okCodex(), empty], [saved("codex", "empty", "profile:empty", "empty@codex.example")]);
    expect(withProfile.empty).toEqual({ reason: "usageUnavailable", copy: "Usage unavailable." });
    expect(cards([okClaude({ windows: [], plan: null, planLabel: null, planCapacity: null, account: null })])[0]).toMatchObject({
      plan: null, empty: { reason: "noWindows", copy: "Claude reported no rate-limit windows." },
    });
    expect(cards([okCodex({ windows: [], currentAccount: false })], [])[0].empty).toEqual({ reason: "noWindows", copy: "Codex reported no rate-limit windows." });
  });

  it("gives a saved read that failed but kept only figures its status, not an empty copy", () => {
    const reason = "Saved usage credential is no longer accepted.";
    const [card] = cards([okCodex({ currentAccount: false, savedProfile: true, status: "failed", message: reason, windows: [], standing: lastObserved("savedRefreshPaused", reason) })]);
    expect(card.status).toEqual({ kind: "savedRefresh", detail: reason });
    expect(card.empty).toBeNull();
  });
});

describe("a card's figures", () => {
  const ZERO: LimitsCredits = { balance: "0", unlimited: false };
  const share = { limit: "25000", used: "8000", usedPercent: 32, resetsAt: "2026-10-01T12:00:00Z", reached: false };
  const renewedShare = { ...share, used: "25000", usedPercent: 100, reached: true, resetsAt: "2026-08-16T10:00:00Z" };
  const spent = { last7Days: 18303.4, last30Days: 20299.7, updatedAt: null };
  it.each([
    ["an own balance of 0 beside a share, which says what the member can use", ZERO, share, null, null],
    ["an own balance of 0 beside a share that has renewed", ZERO, renewedShare, null, null],
    ["an own balance of 0 beside what a member spent", ZERO, null, spent, null],
    ["an own balance that says something beside a share", { balance: "3", unlimited: false }, share, null, { balance: "3", unlimited: false }],
    ["an own balance that says something beside what was spent", { balance: "3", unlimited: false }, null, spent, { balance: "3", unlimited: false }],
    ["an unlimited balance beside a share", { balance: "0", unlimited: true }, share, null, { balance: "0", unlimited: true }],
    ["an unlimited balance alone", { balance: "0", unlimited: true }, null, null, { balance: "0", unlimited: true }],
    ["an own balance of 0 when there is nothing else", ZERO, null, null, ZERO],
    ["no balance at all", null, share, null, null],
  ] as const)("judges %s", (_case, credits, workspaceCredits, creditsSpent, ownBalance) => {
    const { figures } = cards([okCodex({ plan: "self_serve_business_prolite", credits, workspaceCredits, creditsSpent })])[0];
    expect(figures).toMatchObject({ ownBalance, workspaceShare: workspaceCredits, creditsSpent });
  });

  it("shows each Codex card's banked resets", () => {
    const one = { availableCount: 1, nextExpiresAt: null };
    const two = { availableCount: 2, nextExpiresAt: null };
    const codex = cards([okCodex({ resetCredits: two }), staleCodex({ resetCredits: one })]);
    expect(codex.map(card => [card.identity.label, card.figures.bankedResets])).toEqual([
      ["work@codex.example", two],
      ["personal@codex.example", one],
    ]);
  });

  it.each([
    ["whose soonest expiry has passed", { availableCount: 2, nextExpiresAt: "2026-08-17T19:00:00Z" }],
    ["whose soonest expiry is now", { availableCount: 2, nextExpiresAt: NOW }],
    ["that count none", { availableCount: 0, nextExpiresAt: null }],
    ["that were never reported", null],
  ] as const)("drops banked resets %s, since what is left is not known", (_case, resetCredits) => {
    expect(cards([okCodex({ resetCredits })])[0].figures.bankedResets).toBeNull();
  });

  it("shows a paid reset offer on a Codex card that carries one, and never on Claude", () => {
    const offer = { price: { currency: "USD", amountMinorUnits: 800 } };
    expect(cards([okCodex({ resetOffer: offer }), staleCodex()]).map(card => card.figures.paidOffer)).toEqual([offer, null]);
    expect(cards([okClaude({ resetOffer: offer })])[0].figures.paidOffer).toBeNull();
  });
});

describe("a card's subscription badge", () => {
  it("reads Codex's paid-through date per account, refreshing only the signed-in one", () => {
    const term = { activeUntil: "2026-10-10T12:00:00Z", willRenew: false, note: "cancelled" as const, checkedAt: NOW };
    expect(cards([okCodex({ subscription: term }), staleCodex()]).map(card => card.subscription)).toEqual([
      { accountId: "acct-work", poll: true, term },
      { accountId: "acct-personal", poll: false, term: null },
    ]);
    expect(cards([statusOnly("codex", "signedOut", "Sign in with `codex` to see subscription limits.")])[0].subscription).toBeNull();
  });

  it("refreshes by the signed-in state, not by the read: the saved profile's word comes first", () => {
    const poll = (entry: ProviderLimits, active: boolean) =>
      cards([entry], [saved("codex", "work", "acct-work", "work@codex.example", { active })])[0].subscription?.poll;
    expect(poll(okCodex({ currentAccount: false }), true), "the account list already names it").toBe(true);
    expect(poll(okCodex(), false), "the read names it, but its saved login is not the one in use").toBe(false);
  });

  it("is never on a Claude card: Claude Code reports no subscription term", () => {
    expect(cards([okClaude()])[0].subscription).toBeNull();
  });
});

describe("archived accounts", () => {
  const signedIn = okCodex({ account: { id: "acct-me", label: "me@codex.example" } });
  const archivedSaved = staleCodex({ account: { id: "profile:saved", label: "saved@codex.example" }, savedProfile: true, archived: true });
  const archivedHistory = staleCodex({ account: { id: "team", label: "history@codex.example" }, archived: true });
  const visibleSaved = staleCodex({ account: { id: "profile:kept", label: "kept@codex.example" }, savedProfile: true });
  const column = (entries: ProviderLimits[] | undefined, profiles: SavedProfile[] = []) =>
    limitColumn({ provider: "codex", entries, profiles, nativeLogin: null, now: AT_NOW });

  it("splits a provider's cards into the visible ones and the archived ones, profile-only cards included", () => {
    const shown = column([signedIn, archivedSaved, archivedHistory, visibleSaved], [
      saved("codex", "saved", "profile:saved", "saved@codex.example"),
      saved("codex", "unread", "profile:unread", "unread@codex.example", { archived: true }),
      saved("codex", "fresh", "profile:fresh", "fresh@codex.example"),
    ]);
    expect(shown && labels(shown.visible)).toEqual(["me@codex.example", "kept@codex.example", "fresh@codex.example"]);
    expect(shown && labels(shown.archived)).toEqual(["saved@codex.example", "history@codex.example", "unread@codex.example"]);
    expect(shown?.archived.map(card => card.archived)).toEqual([true, true, true]);
    expect(shown?.visible.map(card => card.archived)).toEqual([false, false, false]);
  });

  it("archives a card with the legacy history merged into it, whose ids it takes along", () => {
    const profile = saved("codex", "saved", "profile:user-team", "shared@example.com", { legacyId: "team", identity: { provider: "codex", userId: "user", workspaceId: "team" } });
    const scoped = okCodex({ account: { id: "profile:user-team", label: "shared@example.com" }, currentAccount: false, archived: true });
    const legacy = okCodex({ account: { id: "team", label: "shared@example.com" }, currentAccount: false, archived: true });
    const shown = column([signedIn, scoped, legacy], [profile]);
    expect(shown && labels(shown.archived)).toEqual(["shared@example.com"]);
    expect(shown?.archived[0].account?.forget).toEqual([{ accountId: "team", expectedEmail: "shared@example.com" }, { accountId: "profile:user-team" }]);
  });

  it("never lists the signed-in card as archived", () => {
    const shown = column([{ ...signedIn, archived: true }], [saved("codex", "me", "acct-me", "me@codex.example", { active: true, archived: true })]);
    expect(shown && labels(shown.visible)).toEqual(["me@codex.example"]);
    expect(shown?.archived).toEqual([]);
  });

  it("takes a reading's word over its profile's, which the account list may not have refreshed yet", () => {
    const shown = column([signedIn, visibleSaved], [saved("codex", "kept", "profile:kept", "kept@codex.example", { archived: true })]);
    expect(shown && labels(shown.visible)).toEqual(["me@codex.example", "kept@codex.example"]);
    expect(shown?.archived).toEqual([]);
  });

  it("is nothing until a read answers or a profile is saved", () => {
    expect(column(undefined)).toBeNull();
  });
});

describe("a saved Claude sign-in's end", () => {
  const ended = new Date(AT_NOW - 86_400_000).toISOString();
  const endedMessage = { text: "Claude ended this sign-in on Aug 16. Sign in again to use this account.", tone: "error" };

  it("once passed, is the card's message in red, read or not", () => {
    const unread = saved("claude", "me", "uuid-1", "me@claude.example", { signInEndsAt: ended });
    const [card] = cards(undefined, [unread], AT_NOW, "claude");
    expect(card).toMatchObject({ signIn: { state: "ended" }, empty: null, freshness: { message: endedMessage } });

    const [signedOut] = cards([{ ...okClaude({ windows: [] }), status: "signedOut", message: "Sign in with `claude` to see subscription limits." }], [unread]);
    expect(signedOut).toMatchObject({ signIn: { state: "ended" }, freshness: { message: endedMessage } });
  });

  it("never applies to a Codex card", () => {
    const codex = saved("codex", "me", "acct-me", "me@codex.example", { signInEndsAt: ended });
    expect(cards(undefined, [codex], AT_NOW, "codex")[0]).toMatchObject({
      signIn: null, empty: { reason: "usageUnavailable" }, freshness: { message: null },
    });
  });
});

describe("each provider's card features", () => {
  type Features = { signIn: boolean; term: boolean; paidOffer: boolean };
  const EXPECTED: Record<LimitsProvider, Features> = {
    claude: { signIn: true, term: false, paidOffer: false },
    codex: { signIn: false, term: true, paidOffer: true },
    muse: { signIn: false, term: false, paidOffer: false },
  };

  it.each(LIMITS_PROVIDERS)("gives a %s card only the features its row turns on, from a reading that carries every one", (provider) => {
    const reading: ProviderLimits = {
      provider,
      status: "ok",
      account: { id: "acct-every", label: "every@example.com" },
      currentAccount: true,
      windows: [
        { id: "weekly", label: "Weekly · all models", kind: "weekly", usedPercent: 10, resetsAt: "2026-08-24T13:59:59Z", observedAt: NOW },
        { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 0, resetsAt: null, observedAt: NOW },
      ],
      subscription: { activeUntil: "2026-10-10T12:00:00Z", willRenew: true, checkedAt: NOW },
      resetOffer: { price: { currency: "USD", amountMinorUnits: 800 } },
    };
    const profile = saved(provider, "every", "acct-every", "every@example.com", { active: true, signInEndsAt: new Date(AT_NOW + 2 * 24 * 3_600_000).toISOString() });
    const [card] = cards([reading], [profile], AT_NOW, provider);
    expect({
      signIn: card.signIn !== null,
      term: card.subscription !== null,
      paidOffer: card.figures.paidOffer !== null,
    }).toEqual(EXPECTED[provider]);
  });
});

describe("a card's header tags", () => {
  const savedRefreshPaused = { currentAccount: false, savedProfile: true, status: "unauthenticated" as LimitsStatus, message: "Sign in again.", standing: lastObserved("savedRefreshPaused", "Sign in again.") };
  const renews = { activeUntil: "2026-10-10T12:00:00Z", willRenew: true, checkedAt: NOW };
  const kinds = (card: LimitCard) => cardTags(card).map(tag => tag.kind);

  it("lead with the usage clock, then the renewal tag, then the plan", () => {
    const [codex] = cards([okCodex({ ...savedRefreshPaused, subscription: renews })]);
    expect(kinds(codex)).toEqual(["usageStatus", "term", "plan"]);
  });

  it("lead with the usage clock, then the sign-in tag, then the plan", () => {
    const profile = saved("claude", "uuid-1", "uuid-1", "me@claude.example", { signInEndsAt: new Date(AT_NOW + 2 * 24 * 3_600_000).toISOString() });
    const [claude] = cards([okClaude(savedRefreshPaused)], [profile], AT_NOW, "claude");
    expect(kinds(claude)).toEqual(["usageStatus", "signIn", "plan"]);
  });

  it("lead with a remembered card's status too, and leave the plan out when there is none", () => {
    const [, remembered] = cards([okCodex(), staleCodex({ subscription: renews })]);
    expect(kinds(remembered)).toEqual(["remembered", "term", "plan"]);
    const [planless] = cards([okCodex({ ...savedRefreshPaused, plan: null, planLabel: null, planCapacity: null, subscription: renews })]);
    expect(kinds(planless)).toEqual(["usageStatus", "term"]);
  });
});
