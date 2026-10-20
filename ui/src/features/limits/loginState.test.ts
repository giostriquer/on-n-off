import { describe, expect, it } from "vitest";
import written from "../../../../src-tauri/src/accounts/fixtures/login-states.json";
import type { LoginState, SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import { LIMITS_PROVIDERS, providerLabel, type LimitsProvider } from "$lib/providers";
import { cardActionPlan } from "./cardActionPlan";
import { cardTags } from "./cardTags";
import { limitCards } from "./limitCards";
import { signInPresentation } from "./loginState";
import { NOW, okClaude, okCodex, okMuse } from "./readingFixtures";

const AT_NOW = Date.parse(NOW);
const DAY = 86_400_000;
const at = (days: number) => new Date(AT_NOW + days * DAY).toISOString();
const READING: Record<LimitsProvider, (overrides?: Partial<ProviderLimits>) => ProviderLimits> = { claude: okClaude, codex: okCodex, muse: okMuse };

type Shown = {
  main: string | null;
  useInMenu: boolean;
  tag: { label: string; tone: string; name: string; headline: string } | null;
  message: string | null;
};

function shown(provider: LimitsProvider, loginState: LoginState): Shown {
  const reading = READING[provider]({ currentAccount: false, savedProfile: true, status: "failed", message: "The read's own words." });
  const account = reading.account!;
  const profile: SavedProfile = {
    id: `saved-${provider}`, observationId: account.id, legacyId: provider, identity: { provider, userId: provider, workspaceId: provider },
    email: account.label, label: account.label!, savedAt: NOW, active: false, loginState,
  };
  const card = limitCards({ provider, entries: [reading], profiles: [profile], nativeLogin: { observationId: "someone-else", settled: true }, now: AT_NOW })!
    .find(candidate => candidate.account?.id === account.id)!;
  const plan = cardActionPlan({
    card, now: AT_NOW, checking: false, archiving: false, removing: false, pendingSpend: null,
    accounts: { controls: "controls", blocked: false, signingIn: null, runningClients: null },
  });
  const tag = cardTags(card).find(candidate => candidate.kind === "signIn");
  return {
    main: plan.footer?.main?.action ?? null,
    useInMenu: plan.menu.some(entry => entry.action === "useAccount"),
    tag: tag?.kind === "signIn" ? { label: tag.signIn.label, tone: tag.signIn.tone, name: tag.signIn.name, headline: tag.signIn.headline } : null,
    message: card.freshness.message?.text ?? null,
  };
}

const AGAIN = "Sign in again to use this account.";
const exactly = (text: string) => expect.stringMatching(new RegExp(`^${text}, \\d{1,2}:\\d{2}`));

const TABLE: [string, LoginState, (name: string) => Shown][] = [
  ["usable", { state: "usable" }, () => ({ main: "useAccount", useInMenu: false, tag: null, message: null })],
  ["ending in more than a week", { state: "ending", endsAt: at(20) }, () => ({ main: "useAccount", useInMenu: false, tag: null, message: null })],
  ["ending within the week", { state: "ending", endsAt: at(3) }, name => ({
    main: "useAccount", useInMenu: false, message: null,
    tag: { label: "Sign-in ends Aug 20", tone: "warning", name: `${name} sign-in ends Aug 20`, headline: exactly(`${name} ends this sign-in Aug 20, 2026`) },
  })],
  ["ending, its date passed since the account list said so", { state: "ending", endsAt: at(-0.01) }, name => ({
    main: "useAccount", useInMenu: false, message: null,
    tag: { label: "Sign-in ends Aug 17", tone: "warning", name: `${name} sign-in ends Aug 17`, headline: exactly(`${name} ends this sign-in Aug 17, 2026`) },
  })],
  ["ended", { state: "ended", endsAt: at(-2) }, name => ({
    main: "signInAgain", useInMenu: false, message: `${name} ended this sign-in on Aug 15. ${AGAIN}`,
    tag: { label: "Sign-in ended", tone: "expired", name: `${name} sign-in ended Aug 15`, headline: exactly(`${name} ended this sign-in Aug 15, 2026`) },
  })],
  ["refused", { state: "refused" }, name => ({
    main: "signInAgain", useInMenu: true, message: `${name} no longer accepts this saved login. ${AGAIN}`,
    tag: { label: "Sign-in refused", tone: "expired", name: `${name} sign-in refused`, headline: `${name} no longer accepts this saved login` },
  })],
  ["another account", { state: "otherAccount" }, name => ({
    main: "signInAgain", useInMenu: true, message: `This saved login now signs in as a different ${name} account. ${AGAIN}`,
    tag: {
      label: "Other account", tone: "expired", name: `${name} sign-in is another account`,
      headline: `This saved login now signs in as a different ${name} account`,
    },
  })],
  ["missing", { state: "missing" }, () => ({ main: "signIn", useInMenu: false, tag: null, message: null })],
];

describe.each(LIMITS_PROVIDERS)("a saved %s login's state", provider => {
  it.each(TABLE)("%s decides the card's main button, sign-in tag and message", (_state, loginState, expected) => {
    expect(shown(provider, loginState)).toEqual(expected(providerLabel(provider)));
  });
});

describe("the login states the account list writes", () => {
  const MAIN: Record<LoginState["state"], string> = {
    usable: "useAccount", ending: "useAccount", ended: "signInAgain", refused: "signInAgain", otherAccount: "signInAgain", missing: "signIn",
  };

  it("are every state a card reads, each deciding its main button", () => {
    const states = written as LoginState[];
    expect(states.map(login => login.state).sort()).toEqual(Object.keys(MAIN).sort());
    for (const login of states) expect(shown("codex", login).main, login.state).toBe(MAIN[login.state]);
  });
});

describe("a sign-in's end", () => {
  const ending = (days: number) => signInPresentation({ state: "ending", endsAt: at(days) }, "muse", AT_NOW);

  it("stays out of the way until its last seven days", () => {
    expect(ending(7.01)).toBeNull();
    expect(signInPresentation({ state: "ending", endsAt: "not a date" }, "claude", AT_NOW)).toBeNull();
  });

  it("warns through the last seven days, filling as the end nears", () => {
    expect(ending(7)).toMatchObject({ tone: "warning", progress: 0, name: "Muse sign-in ends Aug 24", countdown: "7 days remaining" });
    expect(ending(3.5)).toMatchObject({ progress: 0.5, countdown: "4 days remaining" });
    expect(ending(1)).toMatchObject({ countdown: "1 day remaining" });
    expect(ending(0.5)).toMatchObject({ countdown: "Less than 1 day remaining" });
  });

  it("names the provider, the exact local time and what comes next", () => {
    const soon = signInPresentation({ state: "ending", endsAt: at(3) }, "claude", AT_NOW)!;
    expect(soon.headline).toMatch(/^Claude ends this sign-in Aug 20, 2026, \d{1,2}:\d{2}/);
    expect(soon.headline).not.toMatch(/GMT|UTC/);
    expect(soon.next).toBe("Then Claude Code asks you to sign in again.");
    expect(signInPresentation({ state: "ending", endsAt: at(3) }, "codex", AT_NOW)!.next).toBe("Then Codex CLI asks you to sign in again.");
    expect(ending(3)!.next).toBe("Then Muse Code asks you to sign in again.");
    const ended = signInPresentation({ state: "ended", endsAt: at(-400) }, "codex", AT_NOW)!;
    expect(ended).toMatchObject({ name: "Codex sign-in ended Jul 13, 2025", countdown: null, progress: 0, next: AGAIN });
    expect(ended.headline).toMatch(/^Codex ended this sign-in Jul 13, 2025, \d{1,2}:\d{2}/);
  });
});
