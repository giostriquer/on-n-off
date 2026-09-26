import type { SavedProfile } from "$lib/accountTypes";
import { planLabel, planMultiplier } from "$lib/limitsFormat";
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
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { accountCards } from "./accountCards";
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

/**
 * Where a Claude reset is spent. on-n-off only reports Claude's, and Claude Code spends the reset of
 * whoever it is signed in as, so only the signed-in account's card names the command.
 */
const CLAUDE_RESET_HINT = "/limit-reset in Claude Code";

/**
 * A quota window as a card shows it. The headline window's `note` is empty when the provider
 * reported no reset; a row under it says why instead.
 */
export type CardWindow = LimitWindowPresentation & { id: string; label: string };

/** The figures a card shows under its windows, each already judged worth showing. */
export type CardFigures = {
  /**
   * The account's own credit balance. In a workspace the credits are the workspace's, so the own
   * balance reads 0 beside a share or spending that says what the member actually has; it is left
   * out then.
   */
  ownBalance: LimitsCredits | null;
  workspaceShare: LimitsWorkspaceCredits | null;
  creditsSpent: LimitsCreditsSpent | null;
  /** Unexpired banked resets, with where a reset is spent when this card is where it is spent. */
  bankedResets: { resetCredits: LimitsResetCredits; hint: string | null } | null;
  /** A paid reset Codex is offering; Claude offers none. */
  paidOffer: LimitsResetOffer | null;
};

/** What the card's subscription badge reads: Codex's term and paid-through date, or Claude's status. */
export type CardSubscription =
  | { provider: "codex"; accountId: string; current: boolean; term: LimitsSubscription | null }
  | { provider: "claude"; status: string | null; lastKnown: boolean; checkedAt: string | null };

/** One snapshot to forget: an account id, and for a legacy id the email it must still carry. */
export type ForgetStep = { accountId: string; expectedEmail?: string };

/** The account a card's controls act on. */
export type CardAccount = {
  id: string;
  /** How the controls and their confirmations name the account: its email or label, else its id. */
  name: string;
  profile: SavedProfile | null;
  /** What removing the account deletes, in order: its legacy history first, then its own. */
  forget: ForgetStep[];
  /** The reading a Codex card's banked-reset action spends against; null on Claude and without a reading. */
  codexActions: ProviderLimits | null;
};

/** Everything one Limits card shows, decided once for both the Limits screen and the menu-bar popover. */
export type LimitCard = {
  key: string;
  provider: AgentId;
  identity: {
    /** What the card is called: the saved profile's email, else the read's label, else the provider. */
    label: string;
    ariaLabel: string;
    category: string | null;
  };
  /** Null for a read that names no account, which has no account controls. */
  account: CardAccount | null;
  plan: string | null;
  status: CardStatus | null;
  /** The account in use: the saved profiles' word for it, else the read's. */
  active: boolean;
  headline: CardWindow | null;
  rows: CardWindow[];
  figures: CardFigures;
  empty: { reason: "usageUnavailable" | "noWindows"; copy: string } | null;
  freshness: {
    updatedAt: string | null;
    lastKnown: boolean;
    message: string | null;
    readStatus: LimitsStatus;
  };
  subscription: CardSubscription | null;
};

export type LimitCardsInput = {
  /** The provider whose cards these are. */
  provider: AgentId;
  /** Its readings; undefined until the first read answers. */
  entries: ProviderLimits[] | undefined;
  /** That provider's saved profiles; the popover passes none, as it never opens the vault. */
  profiles: SavedProfile[];
  now: number;
};

/** A card before it is presented: a reading, a saved profile, or both. */
type Slot = { reading: ProviderLimits; profile: SavedProfile | null } | { reading: null; profile: SavedProfile };

/**
 * One provider's cards, in order. Legacy history a verified saved identity replaced is merged away
 * (`accountCards`), and a saved profile no reading answers for is a card of its own. Null while
 * nothing is known: no read has answered and no profile is saved.
 */
export function limitCards({ provider, entries, profiles, now }: LimitCardsInput): LimitCard[] | null {
  if (!entries && profiles.length === 0) return null;
  const { entries: readings, legacyAccounts } = accountCards(entries ?? [], profiles);
  const slots: Slot[] = readings.map(reading => ({
    reading,
    profile: profiles.find(profile => profile.observationId === reading.account?.id) ?? null,
  }));
  for (const profile of profiles) {
    if (!slots.some(slot => accountOf(slot)?.id === profile.observationId)) slots.push({ reading: null, profile });
  }
  return orderSlots(slots, provider, now).map((slot, index) => presentCard(slot, provider, index, legacyAccounts, now));
}

function accountOf(slot: Slot) {
  return slot.reading ? slot.reading.account ?? null : { id: slot.profile.observationId, label: slot.profile.email };
}

/** The read's word on whether this is the signed-in account; a profile-only card has the profile's. */
function isCurrent(slot: Slot): boolean {
  return slot.reading ? slot.reading.currentAccount : slot.profile.active;
}

/**
 * Card order for one provider's accounts: the active login first; then every account with usage
 * left, the one with the most usable capacity first; then the accounts that are out of usage, the
 * one that becomes usable soonest first; accounts whose usage is unknown last. Usage left always
 * outranks waiting for a reset, however soon that reset is.
 *
 * "Most usable capacity" is the card's percentage read in absolute terms: the plan's multiplier
 * weighs it, so a Max ×20 with 30% left (six Pro-sized plans) outranks an untouched Max ×5 (five).
 * The percentage is `usageLeft`'s and the return instant `usableAgainAt`'s, so what counts as a
 * main window and as a renewed one is decided in one place. The sort is stable: equal ranks keep
 * the backend's order, newest observation first, with profile-only cards after them.
 */
function orderSlots(slots: Slot[], provider: AgentId, now: number): Slot[] {
  const ranked = slots.map(slot => ({ slot, rank: cardRank(slot, provider, now) }));
  ranked.sort((a, b) => compare(a.rank[0], b.rank[0]) || compare(a.rank[1], b.rank[1]));
  return ranked.map(({ slot }) => slot);
}

/** The group, then the group's own measure: capacity left (negated, most first) or the return instant. */
type CardRank = [group: number, within: number];

function cardRank(slot: Slot, provider: AgentId, now: number): CardRank {
  if (isCurrent(slot)) return [0, 0];
  const entry = slot.reading;
  const left = entry ? usageLeft(entry, now) : null;
  if (!entry || left === null) return [3, 0];
  if (left > 0) return [1, -left * planMultiplier(entry.plan, provider)];
  return [2, usableAgainAt(entry, now)];
}

/** Total on the infinities too, where subtraction would hand the sort a NaN. */
function compare(a: number, b: number): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** A profile-only card has read nothing, so nothing on it is stale. */
const UNREAD: LimitAccountPresentation = { status: null, message: null, lastKnown: false, updatedAt: null };

type LegacyAccounts = Map<string, { id: string; email: string }[]>;

function presentCard(slot: Slot, provider: AgentId, index: number, legacyAccounts: LegacyAccounts, now: number): LimitCard {
  const { reading, profile } = slot;
  const name = providerLabel(provider);
  const account = accountOf(slot);
  const current = isCurrent(slot);
  const label = profile?.email ?? account?.label ?? null;
  const presentation = reading ? presentLimitAccount(reading, `${name} limits are unavailable.`) : UNREAD;
  const { headline, rest } = reading ? headlineWindow(reading) : { headline: undefined, rest: [] };
  const readStatus = reading?.status ?? "ok";
  const hasWindows = (reading?.windows.length ?? 0) > 0;
  return {
    key: `${current ? "current" : "remembered"}-${account?.id ?? index}`,
    provider,
    identity: {
      label: label ?? name,
      ariaLabel: label ? `${name} limits · ${label}` : `${name} limits`,
      category: profile?.category || null,
    },
    account: account && {
      id: account.id,
      name: label ?? account.id,
      profile,
      forget: [...(legacyAccounts.get(account.id) ?? []).map(({ id, email }) => ({ accountId: id, expectedEmail: email })), { accountId: account.id }],
      codexActions: provider === "codex" ? reading : null,
    },
    plan: planLabel(reading?.plan, provider) || null,
    status: presentation.status,
    active: !!account && (profile?.active ?? current),
    headline: headline ? presentWindow(headline, now) : null,
    rows: rest.map(window => presentRow(window, provider, now)),
    figures: reading ? figures(reading, provider, now) : NO_FIGURES,
    empty: hasWindows || readStatus !== "ok" || presentation.message ? null
      : profile ? { reason: "usageUnavailable", copy: "Usage unavailable." }
      : { reason: "noWindows", copy: `${name} reported no rate-limit windows.` },
    freshness: {
      updatedAt: presentation.updatedAt,
      lastKnown: presentation.lastKnown,
      message: presentation.message,
      readStatus,
    },
    subscription: subscription(provider, account?.id ?? null, current, reading, presentation),
  };
}

/**
 * Codex's badge reads each account's paid-through date, refreshing only the signed-in one's; it needs
 * an account to ask about. Claude's shows the status the read reported, as current as the card is.
 */
function subscription(
  provider: AgentId,
  accountId: string | null,
  current: boolean,
  reading: ProviderLimits | null,
  { lastKnown, updatedAt }: LimitAccountPresentation,
): CardSubscription | null {
  if (provider === "codex") return accountId === null ? null : { provider, accountId, current, term: reading?.subscription ?? null };
  if (provider === "claude") return { provider, status: reading?.subscriptionStatus ?? null, lastKnown, checkedAt: updatedAt };
  return null;
}

function presentWindow(window: LimitWindow, now: number): CardWindow {
  return { id: window.id, label: window.label, ...presentLimitWindow(window, now) };
}

/** A row with no reset says why: a Claude session that has not started yet, or a reset the provider did not report. */
function presentRow(window: LimitWindow, provider: AgentId, now: number): CardWindow {
  const presented = presentWindow(window, now);
  const awaitingFirstMessage = provider === "claude" && window.kind === "session"
    && window.usedPercent === 0 && window.resetsAt == null;
  return { ...presented, note: presented.note || (awaitingFirstMessage ? "Starts with your first message" : "Reset time unavailable") };
}

const NO_FIGURES: CardFigures = { ownBalance: null, workspaceShare: null, creditsSpent: null, bankedResets: null, paidOffer: null };

function figures(reading: ProviderLimits, provider: AgentId, now: number): CardFigures {
  const share = reading.workspaceCredits ?? null;
  const spent = reading.creditsSpent ?? null;
  const credits = reading.credits ?? null;
  const banked = unexpiredBankedResets(reading.resetCredits, now);
  return {
    ownBalance: credits && (!(share || spent) || credits.unlimited || Number(credits.balance) !== 0) ? credits : null,
    workspaceShare: share,
    creditsSpent: spent,
    bankedResets: banked ? { resetCredits: banked, hint: provider === "claude" && reading.currentAccount ? CLAUDE_RESET_HINT : null } : null,
    paidOffer: provider === "codex" ? reading.resetOffer ?? null : null,
  };
}
