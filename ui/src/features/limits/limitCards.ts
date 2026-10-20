import type { SavedProfile } from "$lib/accountTypes";
import type {
  LimitWindow,
  LimitsColumnEntry,
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
import { claudeSignIn, type SignInPresentation } from "./claudeSignIn";
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

export type CardSubscription = { accountId: string; poll: boolean; term: LimitsSubscription | null };

export type ForgetStep = { accountId: string; expectedEmail?: string };

export type CardAccount = {
  id: string;
  name: string;
  profile: SavedProfile | null;
  forget: ForgetStep[];
};

export type SignedIn = "confirmed" | "unconfirmed" | "no";

export type NativeLogin = { observationId: string | null; settled: boolean };

export type LimitCard = {
  key: string;
  provider: LimitsProvider;
  identity: {
    label: string;
    ariaLabel: string;
    category: string | null;
  };
  account: CardAccount | null;
  reading: ProviderLimits | null;
  plan: string | null;
  status: CardStatus | null;
  signedIn: SignedIn;
  leads: boolean;
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
  archived: boolean;
};

export type LimitCardsInput = {
  provider: LimitsProvider;
  entries: LimitsColumnEntry[] | undefined;
  nativeLogin: NativeLogin | null;
  now: number;
};

type Slot = { entry: LimitsColumnEntry; reading: ProviderLimits | null; profile: SavedProfile | null };

export function limitCards({ provider, entries, nativeLogin, now }: LimitCardsInput): LimitCard[] | null {
  if (!entries) return null;
  const slots: Slot[] = entries.map(entry => ({ entry, reading: entry.unread ? null : entry, profile: entry.profile ?? null }));
  return orderSlots(slots, now).map((slot, index) => presentCard(slot, provider, index, nativeLogin, now));
}

export function isSignedIn(card: LimitCard): boolean {
  return card.signedIn !== "no";
}

export type LimitColumn = { visible: LimitCard[]; archived: LimitCard[] };

export function limitColumn(input: LimitCardsInput): LimitColumn | null {
  const cards = limitCards(input);
  return cards && { visible: cards.filter(card => !card.archived), archived: cards.filter(card => card.archived) };
}

function leadsColumn(slot: Slot): boolean {
  return slot.reading ? slot.reading.currentAccount : !!slot.profile?.active;
}

function signedInState(slot: Slot, accountId: string | undefined, nativeLogin: NativeLogin | null): SignedIn {
  if (accountId === undefined) return "no";
  const claimed = slot.profile ? slot.profile.active : !!slot.reading?.currentAccount;
  if (!claimed) return "no";
  if (!nativeLogin) return "confirmed";
  return nativeLogin.settled && nativeLogin.observationId === accountId ? "confirmed" : "unconfirmed";
}

function orderSlots(slots: Slot[], now: number): Slot[] {
  const ranked = slots.map(slot => ({ slot, rank: cardRank(slot, now) }));
  ranked.sort((a, b) => compare(a.rank[0], b.rank[0]) || compare(a.rank[1], b.rank[1]));
  return ranked.map(({ slot }) => slot);
}

type CardRank = [group: number, within: number];

function cardRank(slot: Slot, now: number): CardRank {
  if (leadsColumn(slot)) return [0, 0];
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

function forgetSteps({ entry }: Slot, accountId: string): ForgetStep[] {
  const history = (entry.replaced ?? []).map(({ id, label }) => (label ? { accountId: id, expectedEmail: label } : { accountId: id }));
  return [...history, { accountId }];
}

function presentCard(slot: Slot, provider: LimitsProvider, index: number, nativeLogin: NativeLogin | null, now: number): LimitCard {
  const { entry, reading, profile } = slot;
  const name = providerLabel(provider);
  const features = LIMITS_FEATURES[provider];
  const account = entry.account ?? null;
  const leads = leadsColumn(slot);
  const signedIn = signedInState(slot, account?.id, nativeLogin);
  const label = profile?.email ?? account?.label ?? null;
  const presentation = reading ? presentLimitAccount(reading, `${name} limits are unavailable.`) : UNREAD;
  const { headline, rest } = reading ? headlineWindow(reading) : { headline: undefined, rest: [] };
  const readStatus = reading?.status ?? "ok";
  const hasWindows = (reading?.windows.length ?? 0) > 0;
  const signIn = features.signInDeadline === "claude" ? claudeSignIn(profile?.signInEndsAt, now) : null;
  const message = signIn?.message ? { text: signIn.message, tone: "error" as const }
    : presentation.message === null ? null
    : { text: presentation.message, tone: readStatus === "failed" ? "error" as const : "muted" as const };
  return {
    key: `${leads ? "current" : "remembered"}-${account?.id ?? index}`,
    provider,
    identity: {
      label: label ?? name,
      ariaLabel: label ? `${name} limits · ${label}` : `${name} limits`,
      category: entry.category || null,
    },
    account: account && {
      id: account.id,
      name: label ?? account.id,
      profile,
      forget: forgetSteps(slot, account.id),
    },
    reading,
    plan: reading?.planLabel || null,
    status: presentation.status,
    signedIn,
    leads,
    headline: headline ? presentWindow(headline, now) : null,
    rows: rest.map(window => presentRow(window, now)),
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
    subscription: subscription(provider, account?.id ?? null, signedIn !== "no", reading),
    signIn,
    archived: !!entry.archived,
  };
}

function subscription(
  provider: LimitsProvider,
  accountId: string | null,
  poll: boolean,
  reading: ProviderLimits | null,
): CardSubscription | null {
  if (LIMITS_FEATURES[provider].subscriptionTerm !== "codex" || accountId === null) return null;
  return { accountId, poll, term: reading?.subscription ?? null };
}

function presentWindow(window: LimitWindow, now: number): CardWindow {
  return { id: window.id, label: window.label, ...presentLimitWindow(window, now) };
}

function presentRow(window: LimitWindow, now: number): CardWindow {
  const presented = presentWindow(window, now);
  return { ...presented, note: presented.note || (window.startsOnFirstMessage ? "Starts with your first message" : "Reset time unavailable") };
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
