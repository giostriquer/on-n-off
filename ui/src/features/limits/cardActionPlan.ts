import type { SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import { codexTermEnded } from "./codexSubscriptionTerm";
import { isSignedIn, type CardAccount, type LimitCard } from "./limitCards";
import { LIMITS_FEATURES } from "./limitFeatures";

export type AccountList<C> = {
  controls: C;
  blocked: boolean;
  signingIn: "running" | "canceling" | null;
  runningClients: string[] | null;
};

export type PlanInput<C> = {
  card: LimitCard;
  now: number;
  checking: boolean;
  accounts: AccountList<C> | null;
  archiving: boolean;
  removing: boolean;
  pendingSpend: { dueAt: string } | null;
};

type SavedLogin<C> = { controls: C; profileId: string };

export type CheckNowEntry = { action: "checkNow"; enabled: boolean };
export type ArchiveEntry = { action: "archive"; enabled: boolean; busy: boolean };

export type MenuEntry<C> =
  | CheckNowEntry
  | { action: "category"; enabled: boolean; initial: string }
  | ({ action: "signInAgain"; enabled: boolean } & SavedLogin<C>)
  | ({ action: "removeLogin"; enabled: boolean } & SavedLogin<C>)
  | ArchiveEntry
  | { action: "resetAlert" }
  | { action: "signOut"; enabled: boolean; controls: C }
  | { action: "remove"; enabled: boolean };

export type MainEntry<C> =
  | { action: "cancelSignIn"; enabled: boolean; busy: boolean; controls: C }
  | ({ action: "signInAgain"; enabled: boolean } & SavedLogin<C>)
  | ArchiveEntry
  | { action: "saveAccount"; enabled: boolean; controls: C }
  | { action: "signIn"; enabled: boolean; controls: C; profileId: string | null }
  | ({ action: "useAccount"; enabled: boolean } & SavedLogin<C>);

export type SwitchAlongside<C> = { enabled: boolean; clients: string[] } & SavedLogin<C>;

export type ResetControls = {
  reading: ProviderLimits;
  useBankedReset: { enabled: boolean } | null;
  automaticSpend: { dueAt: string } | null;
};

export type CardFooterPlan<C> = {
  main: MainEntry<C> | null;
  switchAlongside: SwitchAlongside<C> | null;
  resets: ResetControls | null;
};

export type CardActionPlan<C> =
  | { account: null; menu: CheckNowEntry[]; footer: null }
  | { account: CardAccount; menu: MenuEntry<C>[]; footer: CardFooterPlan<C> };

type Shown<C> = {
  card: LimitCard;
  profile: SavedProfile | null;
  accounts: AccountList<C> | null;
  can: boolean;
  signedIn: boolean;
  confirmed: boolean;
  archive: ArchiveEntry;
};

export function hasResetControls(card: LimitCard): boolean {
  return !!card.account && card.reading !== null && LIMITS_FEATURES[card.provider].bankedResets !== null;
}

export function checkNowEntry(checking: boolean): CheckNowEntry {
  return { action: "checkNow", enabled: !checking };
}

export function cardActionPlan<C>({ card, now, checking, accounts, archiving, removing, pendingSpend }: PlanInput<C>): CardActionPlan<C> {
  const checkNow = checkNowEntry(checking);
  if (!card.account) return { account: null, menu: [checkNow], footer: null };
  const can = !(accounts?.blocked ?? false) && !removing;
  const shown: Shown<C> = {
    card, profile: card.account.profile, accounts, can,
    signedIn: isSignedIn(card), confirmed: card.signedIn === "confirmed",
    archive: { action: "archive", enabled: can, busy: archiving },
  };
  const switchAlongside = switchAlongsideOf(shown);
  return {
    account: card.account,
    menu: [checkNow, ...menuOf(shown)],
    footer: { main: mainOf(shown, now), switchAlongside, resets: resetsOf(shown, switchAlongside !== null, pendingSpend) },
  };
}

function menuOf<C>({ card, profile, accounts, can, signedIn, confirmed, archive }: Shown<C>): MenuEntry<C>[] {
  const saved = accounts && profile ? { controls: accounts.controls, profileId: profile.id } : null;
  return [
    { action: "category", enabled: can, initial: card.identity.category ?? "" },
    ...(saved ? [{ action: "signInAgain" as const, enabled: can, ...saved }] : []),
    ...(saved && signedIn ? [{ action: "removeLogin" as const, enabled: can, ...saved }] : []),
    ...(signedIn ? [] : [archive]),
    ...(LIMITS_FEATURES[card.provider].bankedResets ? [{ action: "resetAlert" as const }] : []),
    ...(accounts && signedIn ? [{ action: "signOut" as const, enabled: can && confirmed, controls: accounts.controls }] : []),
    ...(signedIn ? [] : [{ action: "remove" as const, enabled: can }]),
  ];
}

function mainOf<C>({ card, profile, accounts, can, signedIn, confirmed, archive }: Shown<C>, now: number): MainEntry<C> | null {
  if (!accounts) return null;
  const { controls } = accounts;
  if (accounts.signingIn) return { action: "cancelSignIn", enabled: accounts.signingIn === "running", busy: accounts.signingIn === "canceling", controls };
  if (!profile) return signedIn ? { action: "saveAccount", enabled: can && confirmed, controls } : { action: "signIn", enabled: can, controls, profileId: null };
  if (card.signIn?.state === "ended") return { action: "signInAgain", enabled: can, controls, profileId: profile.id };
  if (!signedIn && !card.leads && !profile.needsLogin && codexTermEnded(card.subscription?.term, now)) return archive;
  if (signedIn && !profile.pendingActivation) return null;
  return { action: profile.needsLogin ? "signIn" : "useAccount", enabled: can, controls, profileId: profile.id };
}

function switchAlongsideOf<C>({ profile, accounts, can }: Shown<C>): SwitchAlongside<C> | null {
  if (!accounts?.runningClients || !profile || profile.needsLogin) return null;
  return { enabled: can, clients: accounts.runningClients, controls: accounts.controls, profileId: profile.id };
}

function resetsOf<C>({ card, can, signedIn, confirmed }: Shown<C>, switching: boolean, pendingSpend: { dueAt: string } | null): ResetControls | null {
  if (!card.reading || !hasResetControls(card)) return null;
  const live = card.leads && card.freshness.readStatus === "ok" && card.figures.bankedResets !== null;
  return {
    reading: card.reading,
    useBankedReset: signedIn && live && !switching ? { enabled: can && confirmed } : null,
    automaticSpend: pendingSpend && { dueAt: pendingSpend.dueAt },
  };
}
