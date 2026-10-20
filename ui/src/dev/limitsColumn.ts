import type { SavedProfile } from "$lib/accountTypes";
import type { LimitsColumnEntry, ProviderLimits } from "$lib/limitsTypes";

export function unreadEntry(profile: SavedProfile): LimitsColumnEntry {
  return {
    provider: profile.identity.provider, status: "ok", account: { id: profile.observationId, legacyId: profile.legacyId, label: profile.email },
    currentAccount: false, windows: [], unread: true, archived: !!profile.archived, profile,
  };
}

const PROFILE_KEY = "profile:";

function email(label: string | null | undefined): string | null {
  return label?.trim().replace(/[A-Z]/g, letter => letter.toLowerCase()) || null;
}

export function supersedes(scoped: ProviderLimits, legacy: ProviderLimits): boolean {
  const [now, before] = [scoped.account, legacy.account];
  return !!now && !!before && !legacy.currentAccount && scoped.provider === legacy.provider && scoped.status === "ok"
    && now.id.startsWith(PROFILE_KEY) && !before.id.startsWith(PROFILE_KEY) && now.legacyId === before.id
    && email(now.label) !== null && email(now.label) === email(before.label);
}

export function asColumn(read: ProviderLimits[], profiles: SavedProfile[], categories: Readonly<Record<string, string>> = {}): LimitsColumnEntry[] {
  const signedIn = read.find(card => card.currentAccount)?.account?.id;
  const savedFor = (id: string | undefined) => profiles.find(profile => profile.observationId === id);
  const cards = read.map(card => {
    const saved = savedFor(card.account?.id);
    return card.account && !card.account.legacyId && saved ? { ...card, account: { ...card.account, legacyId: saved.legacyId } } : card;
  });
  const facts = (id: string | undefined) => {
    const saved = savedFor(id);
    return {
      ...(saved ? { profile: { ...saved, active: id === signedIn } } : {}),
      ...(id && categories[id] ? { category: categories[id] } : {}),
    };
  };
  const shown = cards.filter(card => !cards.some(scoped => supersedes(scoped, card))).map((card): LimitsColumnEntry => {
    const replaced = cards.filter(legacy => supersedes(card, legacy)).flatMap(legacy => (legacy.account ? [legacy.account] : []));
    return { ...card, ...facts(card.account?.id), ...(replaced.length ? { replaced } : {}) };
  });
  const unread = profiles.filter(profile => !cards.some(card => card.account?.id === profile.observationId))
    .map(profile => ({ ...unreadEntry({ ...profile, active: false }), ...facts(profile.observationId) }));
  return [...shown, ...unread];
}
