import type { SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";

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
      entry.status === "ok" && (entry.windows.length > 0 || entry.credits != null) &&
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
