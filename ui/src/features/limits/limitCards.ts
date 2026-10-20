import type { SavedProfile } from "$lib/accountTypes";
import type {
  LimitWindow,
  LimitsCredits,
  LimitsCreditsSpent,
  LimitsResetCredits,
  LimitsResetOffer,
  LimitsStatus,
  LimitsSubscription,
  LimitsWorkspaceCredits,
  ProviderLimits,
} from "$lib/limitsTypes";
import { providerLabel, type LimitsProvider } from "$lib/providers";
import { accountCards } from "./accountCards";
import { claudeSignIn, type SignInPresentation } from "./claudeSignIn";
import { codexTermEnded } from "./codexSubscriptionTerm";
import { LIMITS_FEATURES } from "./limitFeatures";
import {
  headlineWindow,
  presentLimitAccount,
  presentLimitWindow,
  unexpiredBankedResets,
  usableAgainAt,
  usageLeft,
  type CardStatus,
  type LimitAccountPresentation,
  type LimitWindowPresentation,
} from "./limitPresentation";

export type CardWindow = LimitWindowPresentation & { id: string; label: string };

export type CardFigures = {
  ownBalance: LimitsCredits | null;
  workspaceShare: LimitsWorkspaceCredits | null;
  creditsSpent: LimitsCreditsSpent | null;
  bankedResets: LimitsResetCredits | null;
  paidOffer: LimitsResetOffer | null;
};

export type CardSubscription = { accountId: string; current: boolean; term: LimitsSubscription | null };

export type ForgetStep = { accountId: string; expectedEmail?: string };

export type CardAccount = {
  id: string;
  name: string;
  profile: SavedProfile | null;
  forget: ForgetStep[];
  resetActions: ProviderLimits | null;
  resetAlert: boolean;
};

export type LimitCard = {
  key: string;
  provider: LimitsProvider;
  identity: {
    label: string;
    ariaLabel: string;
    category: string | null;
  };
  account: CardAccount | null;
  plan: string | null;
  status: CardStatus | null;
  active: boolean;
  headline: CardWindow | null;
  rows: CardWindow[];
  figures: CardFigures;
  empty: { reason: "usageUnavailable" | "noWindows"; copy: string } | null;
  freshness: {
    updatedAt: string | null;
    lastKnown: boolean;
    message: { text: string; tone: "error" | "muted" } | null;
    readStatus: LimitsStatus;
  };
  subscription: CardSubscription | null;
  signIn: SignInPresentation | null;
  signInAgain: boolean;
  archived: boolean;
  archiveInsteadOfUse: boolean;
};

export type LimitCardsInput = {
  provider: LimitsProvider;
  entries: ProviderLimits[] | undefined;
  profiles: SavedProfile[];
  categories?: Readonly<Record<string, string>>;
  now: number;
};

type Slot = { reading: ProviderLimits; profile: SavedProfile | null } | { reading: null; profile: SavedProfile };

export function limitCards({ provider, entries, profiles, categories = {}, now }: LimitCardsInput): LimitCard[] | null {
  if (!entries && profiles.length === 0) return null;
  const { entries: readings, legacyAccounts } = accountCards(entries ?? [], profiles);
  const slots: Slot[] = readings.map(reading => ({
    reading,
    profile: profiles.find(profile => profile.observationId === reading.account?.id) ?? null,
  }));
  for (const profile of profiles) {
    if (!slots.some(slot => accountOf(slot)?.id === profile.observationId)) slots.push({ reading: null, profile });
  }
  return orderSlots(slots, now).map((slot, index) => presentCard(slot, provider, index, legacyAccounts, categories, now));
}

export type LimitColumn = { visible: LimitCard[]; archived: LimitCard[] };

export function limitColumn(input: LimitCardsInput): LimitColumn | null {
  const cards = limitCards(input);
  return cards && { visible: cards.filter(card => !card.archived), archived: cards.filter(card => card.archived) };
}

function accountOf(slot: Slot) {
  return slot.reading ? slot.reading.account ?? null : { id: slot.profile.observationId, label: slot.profile.email };
}

function isCurrent(slot: Slot): boolean {
  return slot.reading ? slot.reading.currentAccount : slot.profile.active;
}

function orderSlots(slots: Slot[], now: number): Slot[] {
  const ranked = slots.map(slot => ({ slot, rank: cardRank(slot, now) }));
  ranked.sort((a, b) => compare(a.rank[0], b.rank[0]) || compare(a.rank[1], b.rank[1]));
  return ranked.map(({ slot }) => slot);
}

type CardRank = [group: number, within: number];

function cardRank(slot: Slot, now: number): CardRank {
  if (isCurrent(slot)) return [0, 0];
  const entry = slot.reading;
  const left = entry ? usageLeft(entry, now) : null;
  if (!entry || left === null) return [3, 0];
  if (left > 0) return [1, -left * (entry.planCapacity ?? 1)];
  return [2, usableAgainAt(entry, now)];
}

function compare(a: number, b: number): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

const UNREAD: LimitAccountPresentation = { status: null, message: null, lastKnown: false, updatedAt: null };

type LegacyAccounts = Map<string, { id: string; email: string }[]>;

function presentCard(slot: Slot, provider: LimitsProvider, index: number, legacyAccounts: LegacyAccounts, categories: Readonly<Record<string, string>>, now: number): LimitCard {
  const { reading, profile } = slot;
  const name = providerLabel(provider);
  const features = LIMITS_FEATURES[provider];
  const account = accountOf(slot);
  const current = isCurrent(slot);
  const label = profile?.email ?? account?.label ?? null;
  const presentation = reading ? presentLimitAccount(reading, `${name} limits are unavailable.`) : UNREAD;
  const { headline, rest } = reading ? headlineWindow(reading) : { headline: undefined, rest: [] };
  const readStatus = reading?.status ?? "ok";
  const hasWindows = (reading?.windows.length ?? 0) > 0;
  const cardSubscription = subscription(provider, account?.id ?? null, current, reading);
  const active = !!account && (profile?.active ?? current);
  const signIn = features.signInDeadline === "claude" ? claudeSignIn(profile?.signInEndsAt, now) : null;
  const message = signIn?.message ? { text: signIn.message, tone: "error" as const }
    : presentation.message === null ? null
    : { text: presentation.message, tone: readStatus === "failed" ? "error" as const : "muted" as const };
  return {
    key: `${current ? "current" : "remembered"}-${account?.id ?? index}`,
    provider,
    identity: {
      label: label ?? name,
      ariaLabel: label ? `${name} limits · ${label}` : `${name} limits`,
      category: (account && categories[account.id]) || null,
    },
    account: account && {
      id: account.id,
      name: label ?? account.id,
      profile,
      forget: [...(legacyAccounts.get(account.id) ?? []).map(({ id, email }) => ({ accountId: id, expectedEmail: email })), { accountId: account.id }],
      resetActions: features.bankedResets === "codex" ? reading : null,
      resetAlert: features.bankedResets === "codex",
    },
    plan: reading?.planLabel || null,
    status: presentation.status,
    active,
    headline: headline ? presentWindow(headline, now) : null,
    rows: rest.map(window => presentWindow(window, now)),
    figures: reading ? figures(reading, provider, now) : NO_FIGURES,
    empty: hasWindows || readStatus !== "ok" || message ? null
      : profile ? { reason: "usageUnavailable", copy: "Usage unavailable." }
      : { reason: "noWindows", copy: `${name} reported no rate-limit windows.` },
    freshness: {
      updatedAt: presentation.updatedAt,
      lastKnown: presentation.lastKnown,
      message,
      readStatus,
    },
    subscription: cardSubscription,
    signIn,
    signInAgain: signIn?.state === "ended" && !!profile,
    archived: !current && !!(reading ? reading.archived : profile?.archived),
    archiveInsteadOfUse: !current && !active && !!profile && !profile.needsLogin && subscriptionEnded(cardSubscription, now),
  };
}

function subscriptionEnded(subscription: CardSubscription | null, now: number): boolean {
  return subscription !== null && codexTermEnded(subscription.term, now);
}

function subscription(
  provider: LimitsProvider,
  accountId: string | null,
  current: boolean,
  reading: ProviderLimits | null,
): CardSubscription | null {
  if (LIMITS_FEATURES[provider].subscriptionTerm !== "codex" || accountId === null) return null;
  return { accountId, current, term: reading?.subscription ?? null };
}

function presentWindow(window: LimitWindow, now: number): CardWindow {
  return { id: window.id, label: window.label, ...presentLimitWindow(window, now) };
}

const NO_FIGURES: CardFigures = { ownBalance: null, workspaceShare: null, creditsSpent: null, bankedResets: null, paidOffer: null };

function figures(reading: ProviderLimits, provider: LimitsProvider, now: number): CardFigures {
  const share = reading.workspaceCredits ?? null;
  const spent = reading.creditsSpent ?? null;
  const credits = reading.credits ?? null;
  const banked = unexpiredBankedResets(reading.resetCredits, now);
  return {
    ownBalance: credits && (!(share || spent) || credits.unlimited || Number(credits.balance) !== 0) ? credits : null,
    workspaceShare: share,
    creditsSpent: spent,
    bankedResets: banked,
    paidOffer: LIMITS_FEATURES[provider].paidResetOffer === "codex" ? reading.resetOffer ?? null : null,
  };
}
