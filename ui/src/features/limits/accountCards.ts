import type { SavedProfile } from "$lib/accountTypes";
import { planMultiplier } from "$lib/limitsFormat";
import type { ProviderLimits } from "$lib/limitsTypes";
import { hasObservations, usableAgainAt, usageLeft } from "./limitPresentation";

const emailKey = (email?: string | null) => email?.trim().toLowerCase() || null;

/** Join history to verified saved identities, even when an older writer drops legacyId.
 * Only an actual scoped observation replaces legacy history; never transfer its quotas.
 */
export function accountCards(entries: ProviderLimits[], profiles: SavedProfile[]) {
  const replacements = profiles.flatMap(profile => {
    const email = emailKey(profile.email);
    if (!email) return [];
    const scoped = entries.find(entry => entry.provider === profile.identity.provider &&
      entry.account?.id === profile.observationId && entry.account?.id.startsWith("profile:") &&
      entry.status === "ok" && hasObservations(entry) &&
      emailKey(entry.account.label) === email);
    if (!scoped?.account) return [];
    const legacyId = profile.identity.provider === "codex" ? profile.identity.workspaceId :
      profile.identity.provider === "claude" ? profile.identity.userId : null;
    return legacyId ? [{ provider: profile.identity.provider, legacyId, email, scopedId: scoped.account.id }] : [];
  });
  const legacyAccounts = new Map<string, { id: string; email: string }[]>();
  const visible = entries.filter(entry => {
    if (entry.currentAccount || !entry.account || entry.account.id.startsWith("profile:")) return true;
    const matches = replacements.filter(replacement =>
      replacement.provider === entry.provider && replacement.legacyId === entry.account?.id &&
      replacement.email === emailKey(entry.account?.label));
    for (const match of matches) {
      legacyAccounts.set(match.scopedId, [...(legacyAccounts.get(match.scopedId) ?? []), { id: entry.account.id, email: match.email }]);
    }
    return matches.length === 0;
  });
  return { entries: visible, legacyAccounts };
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
 * the backend's order, newest observation first.
 */
export function orderAccountCards(entries: ProviderLimits[], now: number): ProviderLimits[] {
  const ranked = entries.map(entry => ({ entry, rank: cardRank(entry, now) }));
  ranked.sort((a, b) => compare(a.rank[0], b.rank[0]) || compare(a.rank[1], b.rank[1]));
  return ranked.map(({ entry }) => entry);
}

/** The group, then the group's own measure: capacity left (negated, most first) or the return instant. */
type CardRank = [group: number, within: number];

function cardRank(entry: ProviderLimits, now: number): CardRank {
  if (entry.currentAccount) return [0, 0];
  const left = usageLeft(entry, now);
  if (left === null) return [3, 0];
  if (left > 0) return [1, -left * planMultiplier(entry.plan, entry.provider)];
  return [2, usableAgainAt(entry, now)];
}

/** Total on the infinities too, where subtraction would hand the sort a NaN. */
function compare(a: number, b: number): number {
  return a < b ? -1 : a > b ? 1 : 0;
}
