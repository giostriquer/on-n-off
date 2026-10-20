import type { ProviderLimits } from "$lib/limitsTypes";
import { codexTermEnded } from "./codexSubscriptionTerm";
import type { CardAccount, LimitCard } from "./limitCards";
import { LIMITS_FEATURES } from "./limitFeatures";

export type AccountListState = {
  blocked: boolean;
  signingIn: "running" | "canceling" | null;
  runningClients: boolean;
};

export type CardActionInput = {
  card: LimitCard | null;
  now: number;
  checking: boolean;
  accounts: AccountListState | null;
  archiving: boolean;
  removing: boolean;
  pendingSpend: boolean;
};

export type MenuAction = "checkNow" | "category" | "signInAgain" | "removeLogin" | "archive" | "resetAlert" | "signOut" | "remove";
export type MainAction = "cancelSignIn" | "signInAgain" | "archive" | "saveAccount" | "signIn" | "useAccount";
export type ArchivedRowAction = "unarchive" | "remove";

export type PlanEntry<A extends string> = { action: A; enabled: boolean; busy: boolean };

export type ResetControls = {
  useBankedReset: PlanEntry<"useBankedReset"> | null;
  cancelAutomaticSpend: PlanEntry<"cancelAutomaticSpend"> | null;
};

export type CardActionPlan = {
  menu: PlanEntry<MenuAction>[];
  footer: {
    main: PlanEntry<MainAction> | null;
    switchAlongside: PlanEntry<"switchAnyway"> | null;
    resets: ResetControls | null;
  };
};

function entry<A extends string>(action: A, enabled: boolean, busy = false): PlanEntry<A> {
  return { action, enabled, busy };
}

type ReadAccountCard = LimitCard & { account: CardAccount; reading: ProviderLimits };

export function hasResetControls(card: LimitCard | null): card is ReadAccountCard {
  return !!card?.account && card.reading !== null && LIMITS_FEATURES[card.provider].bankedResets !== null;
}

export function cardActionPlan({ card, now, checking, accounts, archiving, removing, pendingSpend }: CardActionInput): CardActionPlan {
  const checkNow = entry("checkNow", !checking);
  if (!card?.account) return { menu: [checkNow], footer: { main: null, switchAlongside: null, resets: null } };
  const { profile } = card.account;
  const can = !(accounts?.blocked ?? false) && !removing;
  const signedIn = card.signedIn !== "no";
  const confirmed = card.signedIn === "confirmed";
  const archive = entry("archive", can, archiving);
  const saved = accounts !== null && profile !== null;
  const menu: PlanEntry<MenuAction>[] = [
    checkNow,
    entry("category", can),
    ...(saved ? [entry("signInAgain", can)] : []),
    ...(saved && signedIn ? [entry("removeLogin", can)] : []),
    ...(signedIn ? [] : [archive]),
    ...(LIMITS_FEATURES[card.provider].bankedResets ? [entry("resetAlert", true)] : []),
    ...(accounts && signedIn ? [entry("signOut", can && confirmed)] : []),
    ...(signedIn ? [] : [entry("remove", can)]),
  ];
  const archiveInsteadOfUse = !signedIn && !card.reading?.currentAccount && !!profile && !profile.needsLogin
    && codexTermEnded(card.subscription?.term, now);
  const switchAlongside = accounts?.runningClients && profile && !profile.needsLogin ? entry("switchAnyway", can) : null;
  return { menu, footer: { main: main(), switchAlongside, resets: resets(card) } };

  function main(): PlanEntry<MainAction> | null {
    if (!accounts) return null;
    if (accounts.signingIn) return entry("cancelSignIn", accounts.signingIn === "running", accounts.signingIn === "canceling");
    if (profile && card?.signIn?.state === "ended") return entry("signInAgain", can);
    if (archiveInsteadOfUse) return archive;
    if (!profile) return signedIn ? entry("saveAccount", can && confirmed) : entry("signIn", can);
    if (signedIn && !profile.pendingActivation) return null;
    return profile.needsLogin ? entry("signIn", can) : entry("useAccount", can);
  }

  function resets(shown: LimitCard): ResetControls | null {
    if (!hasResetControls(shown)) return null;
    const live = shown.reading.currentAccount && shown.reading.status === "ok" && shown.figures.bankedResets !== null;
    return {
      useBankedReset: signedIn && live && !switchAlongside ? entry("useBankedReset", can && confirmed) : null,
      cancelAutomaticSpend: pendingSpend ? entry("cancelAutomaticSpend", true) : null,
    };
  }
}

export function archivedRowPlan(card: LimitCard, { blocked, working }: { blocked: boolean; working: boolean }): PlanEntry<ArchivedRowAction>[] {
  if (!card.account) return [];
  const enabled = !blocked && !working;
  return [entry("unarchive", enabled), entry("remove", enabled)];
}
