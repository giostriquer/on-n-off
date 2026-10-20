import { describe, expect, it } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { LimitsColumnEntry } from "$lib/limitsTypes";
import { LIMITS_PROVIDERS, type LimitsProvider } from "$lib/providers";
import { cardActionPlan, checkNowEntry, type AccountList, type CardActionPlan } from "./cardActionPlan";
import { limitCards, type LimitCard, type NativeLogin } from "./limitCards";
import { NOW, okClaude, okCodex, okMuse, staleCodex, withSavedProfiles } from "./readingFixtures";

const AT_NOW = Date.parse(NOW);
const DAY = 86_400_000;
const before = (days: number) => new Date(AT_NOW - days * DAY).toISOString();
const BANKED = { availableCount: 1, nextExpiresAt: null };

const READING: Record<LimitsProvider, (overrides?: Partial<LimitsColumnEntry>) => LimitsColumnEntry> = { claude: okClaude, codex: okCodex, muse: okMuse };

function reading(provider: LimitsProvider, overrides: Partial<LimitsColumnEntry> = {}): LimitsColumnEntry {
  return READING[provider]({ resetCredits: BANKED, ...overrides });
}

function accountId(provider: LimitsProvider): string {
  return READING[provider]().account!.id;
}

function profile(provider: LimitsProvider, overrides: Partial<SavedProfile> = {}): SavedProfile {
  const email = READING[provider]().account!.label!;
  return {
    id: `saved-${provider}`, observationId: accountId(provider), legacyId: provider, identity: { provider, userId: provider, workspaceId: provider },
    email, label: email, savedAt: NOW, active: false, loginState: { state: "usable" }, ...overrides,
  };
}

const CONTROLS = "the account controls";
const LIST: AccountList<string> = { controls: CONTROLS, blocked: false, signingIn: null, runningClients: null };
const RUNNING = ["Acme Studio (codex)"];
const SPEND = { dueAt: "2026-08-17T20:08:00Z" };

type Setup = {
  entries: LimitsColumnEntry[];
  profiles?: SavedProfile[];
  nativeLogin?: NativeLogin | null;
  accounts?: AccountList<string> | null;
  pendingSpend?: { dueAt: string } | null;
};

function card(provider: LimitsProvider, { entries, profiles = [], nativeLogin = { observationId: "someone-else", settled: true } }: Setup): LimitCard {
  const cards = limitCards({ provider, entries: withSavedProfiles(entries, profiles), nativeLogin, now: AT_NOW });
  return cards!.find(shown => shown.account?.id === accountId(provider))!;
}

function planFor(shown: LimitCard, setup: Setup, overrides: { checking?: boolean; archiving?: boolean; removing?: boolean } = {}): CardActionPlan<string> {
  return cardActionPlan({
    card: shown, now: AT_NOW, checking: false, archiving: false, removing: false,
    accounts: setup.accounts === undefined ? LIST : setup.accounts, pendingSpend: setup.pendingSpend ?? null, ...overrides,
  });
}

function plan(provider: LimitsProvider, setup: Setup, overrides: { checking?: boolean; archiving?: boolean; removing?: boolean } = {}): CardActionPlan<string> {
  return planFor(card(provider, setup), setup, overrides);
}

function described({ action, enabled = true, busy = false }: { action: string; enabled?: boolean; busy?: boolean }): string {
  return `${action}${enabled ? "" : "-"}${busy ? "*" : ""}`;
}

function describe_(shown: CardActionPlan<string>): { menu: string; footer: string } {
  const menu = shown.menu.map(described).join(" ");
  if (shown.footer === null) return { menu, footer: "" };
  const { main, switchAlongside, resets } = shown.footer;
  const footer = [
    main && described(main),
    switchAlongside && described({ action: "switchAnyway", ...switchAlongside }),
    resets?.useBankedReset && described({ action: "useBankedReset", ...resets.useBankedReset }),
    resets?.automaticSpend && "automaticSpend",
  ];
  return { menu, footer: footer.filter(Boolean).join(" ") };
}

const STATES = {
  "current confirmed": (provider: LimitsProvider): Setup => ({
    entries: [reading(provider)], profiles: [profile(provider, { active: true })], nativeLogin: { observationId: accountId(provider), settled: true },
  }),
  "current unconfirmed": (provider: LimitsProvider): Setup => ({ entries: [reading(provider)] }),
  "current unsaved": (provider: LimitsProvider): Setup => ({ entries: [reading(provider)], nativeLogin: { observationId: accountId(provider), settled: true } }),
  saved: (provider: LimitsProvider): Setup => ({ entries: [reading(provider, { currentAccount: false, savedProfile: true })], profiles: [profile(provider)] }),
  "needs login": (provider: LimitsProvider): Setup => ({ entries: [reading(provider, { currentAccount: false })], profiles: [profile(provider, { loginState: { state: "missing" } })] }),
  "sign-in ended": (provider: LimitsProvider): Setup => ({
    entries: [reading(provider, { currentAccount: false, savedProfile: true })], profiles: [profile(provider, { loginState: { state: "ended", endsAt: before(1) } })],
  }),
  "term ended": (provider: LimitsProvider): Setup => ({
    entries: [reading(provider, { currentAccount: false, savedProfile: true, subscription: { activeUntil: before(2), willRenew: false, checkedAt: NOW } })],
    profiles: [profile(provider)],
  }),
  "signing in": (provider: LimitsProvider): Setup => ({
    entries: [reading(provider, { currentAccount: false, savedProfile: true })], profiles: [profile(provider)], accounts: { ...LIST, signingIn: "running", blocked: true },
  }),
  remembered: (provider: LimitsProvider): Setup => ({ entries: [reading(provider, { currentAccount: false })] }),
  "pending automatic spend": (provider: LimitsProvider): Setup => ({
    entries: [reading(provider, { currentAccount: false, savedProfile: true })], profiles: [profile(provider)],
    accounts: { ...LIST, runningClients: RUNNING }, pendingSpend: SPEND,
  }),
  "no account manager": (provider: LimitsProvider): Setup => ({ entries: [reading(provider)], nativeLogin: null, accounts: null, pendingSpend: SPEND }),
} satisfies Record<string, (provider: LimitsProvider) => Setup>;

type State = keyof typeof STATES;
type Expected = { menu: string; footer: string };

const UNHELD = ["checkNow", "resetAlert", "cancelSignIn", "automaticSpend"];
const SIGNED_IN = "checkNow category signInAgain removeLogin signOut";
const SAVED = "checkNow category signInAgain archive remove";
const CODEX_SAVED = "checkNow category signInAgain archive resetAlert remove";

const TABLE: Record<LimitsProvider, Record<State, Expected>> = {
  claude: {
    "current confirmed": { menu: SIGNED_IN, footer: "" },
    "current unconfirmed": { menu: "checkNow category signOut-", footer: "saveAccount-" },
    "current unsaved": { menu: "checkNow category signOut", footer: "saveAccount" },
    saved: { menu: SAVED, footer: "useAccount" },
    "needs login": { menu: SAVED, footer: "signIn" },
    "sign-in ended": { menu: SAVED, footer: "signInAgain" },
    "term ended": { menu: SAVED, footer: "useAccount" },
    "signing in": { menu: "checkNow category- signInAgain- archive- remove-", footer: "cancelSignIn" },
    remembered: { menu: "checkNow category archive remove", footer: "signIn" },
    "pending automatic spend": { menu: SAVED, footer: "useAccount switchAnyway" },
    "no account manager": { menu: "checkNow category", footer: "" },
  },
  codex: {
    "current confirmed": { menu: "checkNow category signInAgain removeLogin resetAlert signOut", footer: "useBankedReset" },
    "current unconfirmed": { menu: "checkNow category resetAlert signOut-", footer: "saveAccount- useBankedReset-" },
    "current unsaved": { menu: "checkNow category resetAlert signOut", footer: "saveAccount useBankedReset" },
    saved: { menu: CODEX_SAVED, footer: "useAccount" },
    "needs login": { menu: CODEX_SAVED, footer: "signIn" },
    "sign-in ended": { menu: CODEX_SAVED, footer: "signInAgain" },
    "term ended": { menu: CODEX_SAVED, footer: "archive" },
    "signing in": { menu: "checkNow category- signInAgain- archive- resetAlert remove-", footer: "cancelSignIn" },
    remembered: { menu: "checkNow category archive resetAlert remove", footer: "signIn" },
    "pending automatic spend": { menu: CODEX_SAVED, footer: "useAccount switchAnyway automaticSpend" },
    "no account manager": { menu: "checkNow category resetAlert", footer: "useBankedReset automaticSpend" },
  },
  muse: {
    "current confirmed": { menu: SIGNED_IN, footer: "" },
    "current unconfirmed": { menu: "checkNow category signOut-", footer: "saveAccount-" },
    "current unsaved": { menu: "checkNow category signOut", footer: "saveAccount" },
    saved: { menu: SAVED, footer: "useAccount" },
    "needs login": { menu: SAVED, footer: "signIn" },
    "sign-in ended": { menu: SAVED, footer: "signInAgain" },
    "term ended": { menu: SAVED, footer: "useAccount" },
    "signing in": { menu: "checkNow category- signInAgain- archive- remove-", footer: "cancelSignIn" },
    remembered: { menu: "checkNow category archive remove", footer: "signIn" },
    "pending automatic spend": { menu: SAVED, footer: "useAccount switchAnyway" },
    "no account manager": { menu: "checkNow category", footer: "" },
  },
};

describe.each(LIMITS_PROVIDERS)("the %s card action plan", provider => {
  it.each(Object.keys(STATES) as State[])("for a card %s", state => {
    expect(describe_(plan(provider, STATES[state](provider)))).toEqual(TABLE[provider][state]);
  });

  it.each(Object.keys(STATES) as State[])("holds every account action on a card %s while the account list cannot act or its removal runs", state => {
    const setup = STATES[state](provider);
    const runs = [plan(provider, setup, { removing: true })];
    if (setup.accounts !== null) runs.push(plan(provider, { ...setup, accounts: { ...(setup.accounts ?? LIST), blocked: true } }));
    for (const shown of runs) {
      const { menu, footer } = describe_(shown);
      const enabled = `${menu} ${footer}`.split(" ").filter(token => token && !token.endsWith("-"));
      expect(enabled.filter(token => !UNHELD.includes(token))).toEqual([]);
    }
  });

  it("disables only Check limits now while the provider is being checked", () => {
    expect(describe_(plan(provider, STATES.saved(provider), { checking: true })).menu.split(" ").filter(entry => entry.endsWith("-"))).toEqual(["checkNow-"]);
  });

  it("offers a card with no account only Check limits now, as it offers a card not yet read", () => {
    const anonymous = limitCards({ provider, entries: [{ provider, status: "signedOut", message: null, currentAccount: true, windows: [] }], nativeLogin: null, now: AT_NOW })![0];
    const shown = planFor(anonymous, { entries: [], pendingSpend: SPEND });
    expect(shown).toEqual({ account: null, menu: [{ action: "checkNow", enabled: true }], footer: null });
    expect([checkNowEntry(false), checkNowEntry(true)]).toEqual([{ action: "checkNow", enabled: true }, { action: "checkNow", enabled: false }]);
  });
});

describe("a card's one signed-in state", () => {
  const signedIn = (setup: Setup) => card("codex", setup).signedIn;

  it("is the saved profile's word when there is one, else the read's, confirmed only once the account list names the account and has settled", () => {
    expect(signedIn(STATES["current confirmed"]("codex"))).toBe("confirmed");
    expect(signedIn({ ...STATES["current confirmed"]("codex"), nativeLogin: { observationId: accountId("codex"), settled: false } })).toBe("unconfirmed");
    expect(signedIn(STATES["current unconfirmed"]("codex"))).toBe("unconfirmed");
    expect(signedIn(STATES.saved("codex"))).toBe("no");
    expect(signedIn({ entries: [], profiles: [profile("codex")], nativeLogin: { observationId: accountId("codex"), settled: true } }), "a saved login no read answered for").toBe("no");
    expect(signedIn(STATES.remembered("codex"))).toBe("no");
  });

  it("takes the read's word as confirmed where there is no account list to ask", () => {
    expect(signedIn({ entries: [reading("codex")], nativeLogin: null })).toBe("confirmed");
  });
});

describe("Use banked reset", () => {
  const live = STATES["current confirmed"]("codex");
  const offered = (setup: Setup) => plan("codex", setup).footer?.resets?.useBankedReset ?? null;

  it("is offered only on the signed-in account's live reading that has a reset banked", () => {
    expect(offered(live)).toEqual({ enabled: true });
    for (const entries of [
      [reading("codex", { currentAccount: false })],
      [reading("codex", { status: "failed" })],
      [reading("codex", { resetCredits: { availableCount: 0, nextExpiresAt: null } })],
      [reading("codex", { resetCredits: null })],
      [reading("codex", { resetCredits: { availableCount: 1, nextExpiresAt: before(0.001) } })],
    ]) expect(offered({ ...live, entries })).toBeNull();
  });

  it("gives way to a Switch anyway confirmation on its card, which a waiting automatic reset's Cancel does not", () => {
    const pending = { ...live, profiles: [profile("codex", { active: true, pendingActivation: true })], accounts: { ...LIST, runningClients: RUNNING }, pendingSpend: SPEND };
    expect(describe_(plan("codex", pending)).footer).toBe("useAccount switchAnyway automaticSpend");
  });

  it("has no reset controls on a card whose read named no account, nor on a saved login no read answered for", () => {
    const anonymous = limitCards({ provider: "codex", entries: [reading("codex", { account: null })], nativeLogin: null, now: AT_NOW })![0];
    expect(planFor(anonymous, { entries: [], accounts: null, pendingSpend: SPEND }).footer).toBeNull();
    const unread = plan("codex", { entries: [], profiles: [profile("codex")], pendingSpend: SPEND });
    expect(unread.footer?.resets).toBeNull();
  });
});

describe("Archive account", () => {
  it("is one action, in the menu and in the footer's place of Use account, busy in both while it runs", () => {
    const shown = describe_(plan("codex", STATES["term ended"]("codex"), { archiving: true }));
    expect(shown).toEqual({ menu: "checkNow category signInAgain archive* resetAlert remove", footer: "archive*" });
  });
});

describe("the footer of a saved account whose subscription ended", () => {
  const PASSED = "2026-08-10T00:00:00Z";
  const AHEAD = "2026-09-10T00:00:00Z";
  const term = (willRenew: boolean, activeUntil: string) => ({ activeUntil, willRenew, checkedAt: NOW });
  const codexSaved = (subscription: LimitsColumnEntry["subscription"], overrides: Partial<LimitsColumnEntry> = {}) =>
    staleCodex({ account: { id: "profile:saved", label: "saved@codex.example" }, savedProfile: true, subscription, ...overrides });
  const profileOf = (overrides: Partial<SavedProfile> = {}): SavedProfile => ({
    id: "saved", observationId: "profile:saved", legacyId: "saved", identity: { provider: "codex", userId: "saved", workspaceId: "saved" },
    email: "saved@codex.example", label: "saved@codex.example", savedAt: NOW, active: false, loginState: { state: "usable" }, ...overrides,
  });

  it.each<[string, boolean, LimitsColumnEntry, SavedProfile | null]>([
    ["Codex: will not renew and its date has passed", true, codexSaved(term(false, PASSED)), profileOf()],
    ["Codex: will not renew, but its date is ahead", false, codexSaved(term(false, AHEAD)), profileOf()],
    ["Codex: renews, though its date has passed", false, codexSaved(term(true, PASSED)), profileOf()],
    ["Codex: no term read", false, codexSaved(null), profileOf()],
    ["Codex: a term whose date cannot be read", false, codexSaved(term(false, "not a date")), profileOf()],
    ["the signed-in account", false, codexSaved(term(false, PASSED), { currentAccount: true }), profileOf({ active: true })],
    ["history with no saved login, whose footer offers Sign in", false, codexSaved(term(false, PASSED), { savedProfile: false }), null],
    ["a saved login that needs sign-in again", false, codexSaved(term(false, PASSED)), profileOf({ loginState: { state: "missing" } })],
  ])("%s → Archive account in place of Use account: %s", (_case, expected, reading, saved) => {
    const shown = limitCards({ provider: "codex", entries: withSavedProfiles([reading], saved ? [saved] : []), nativeLogin: { observationId: "profile:saved", settled: true }, now: AT_NOW })!
      .find(card => card.account?.id === "profile:saved")!;
    expect(planFor(shown, { entries: [] }).footer?.main?.action === "archive").toBe(expected);
  });

  it.each<[string, SavedProfile["loginState"]]>([
    ["refused", { state: "refused" }],
    ["now another account", { state: "otherAccount" }],
  ])("a saved login %s asks to sign in again before it is archived, Archive account kept in the menu", (_case, loginState) => {
    const shown = limitCards({ provider: "codex", entries: withSavedProfiles([codexSaved(term(false, PASSED))], [profileOf({ loginState })]), nativeLogin: { observationId: "profile:saved", settled: true }, now: AT_NOW })!
      .find(card => card.account?.id === "profile:saved")!;
    expect(describe_(planFor(shown, { entries: [] }))).toEqual({ menu: "checkNow category signInAgain archive resetAlert remove", footer: "signInAgain" });
  });
});

describe("Sign in again in place of the footer's action", () => {
  const ended = before(1);
  const claudeProfile = (overrides: Partial<SavedProfile>) => profile("claude", { loginState: { state: "ended", endsAt: ended }, ...overrides });
  const main = (setup: Setup) => plan("claude", setup).footer?.main?.action ?? null;

  it("is offered once a saved sign-in ended, read or not, signed out or never used", () => {
    expect(main({ entries: [], profiles: [claudeProfile({})] })).toBe("signInAgain");
    expect(main({ entries: [{ ...okClaude({ windows: [] }), status: "signedOut", message: null }], profiles: [claudeProfile({})] })).toBe("signInAgain");
    expect(main({ entries: [reading("claude", { currentAccount: false, savedProfile: true, status: "failed" })], profiles: [claudeProfile({ pendingActivation: true })] })).toBe("signInAgain");
  });

  it("gives way to the Cancel of the sign-in it started", () => {
    expect(main({ entries: [], profiles: [claudeProfile({})], accounts: { ...LIST, signingIn: "canceling" } })).toBe("cancelSignIn");
    expect(plan("claude", { entries: [], profiles: [claudeProfile({})], accounts: { ...LIST, signingIn: "canceling" } }).footer?.main)
      .toEqual({ action: "cancelSignIn", enabled: false, busy: true, controls: CONTROLS });
  });
});

describe("each entry", () => {
  it("carries what its handler needs: the account controls, the saved login, the clients still running, the reading, its headline window and the waiting spend", () => {
    const setup = STATES["pending automatic spend"]("codex");
    const shown = card("codex", setup);
    const pending = planFor(shown, setup);
    expect(pending.account?.id).toBe(accountId("codex"));
    expect(shown.headline).not.toBeNull();
    expect(pending.footer).toEqual({
      main: { action: "useAccount", enabled: true, controls: CONTROLS, profileId: "saved-codex" },
      switchAlongside: { enabled: true, clients: RUNNING, controls: CONTROLS, profileId: "saved-codex" },
      resets: { reading: reading("codex", { currentAccount: false, savedProfile: true, profile: profile("codex") }), headline: shown.headline, useBankedReset: null, automaticSpend: SPEND },
    });
    expect(pending.menu.find(entry => entry.action === "signInAgain")).toEqual({ action: "signInAgain", enabled: true, controls: CONTROLS, profileId: "saved-codex" });
    expect(plan("codex", STATES.remembered("codex")).footer?.main).toEqual({ action: "signIn", enabled: true, controls: CONTROLS, profileId: null });
    expect(plan("codex", STATES["needs login"]("codex")).footer?.main).toEqual({ action: "signIn", enabled: true, controls: CONTROLS, profileId: "saved-codex" });
  });

  it("starts the category editor from the card's category", () => {
    const shown = limitCards({ provider: "muse", entries: [reading("muse", { category: "Client A" })], nativeLogin: null, now: AT_NOW })![0];
    expect(planFor(shown, { entries: [] }).menu[1]).toEqual({ action: "category", enabled: true, initial: "Client A" });
  });
});
