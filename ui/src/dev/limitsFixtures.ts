import type { AccountCategories, AccountsReading, SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import { hasLimits, hasSavedAccounts, isAgentId, LIMITS_PROVIDERS, type LimitsProvider } from "$lib/providers";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { AgentId } from "$lib/types";

const NOW = Date.parse("2026-08-24T20:00:00Z");

function at(offsetMinutes: number): string {
  return new Date(NOW + offsetMinutes * 60_000).toISOString();
}

const OBSERVED = at(0);
const REMEMBERED_OBSERVED = at(-4 * 24 * 60 - 12 * 60);

const CLAUDE: ProviderLimits[] = [
  {
    provider: "claude",
    status: "ok",
    account: { id: "claude-1", label: "you@example.com" },
    currentAccount: true,
    plan: "max ×20",
    windows: [
      { id: "weekly_all", label: "Weekly · all models", kind: "weekly", usedPercent: 9, resetsAt: at(6 * 24 * 60 + 12 * 60), observedAt: OBSERVED },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 10, resetsAt: at(3 * 60 + 20), observedAt: OBSERVED },
      { id: "weekly_fable", label: "Weekly · Fable", kind: "model", usedPercent: 16, resetsAt: at(6 * 24 * 60 + 12 * 60), observedAt: OBSERVED },
    ],
  },
];

const CODEX: ProviderLimits[] = [
  {
    provider: "codex",
    status: "ok",
    account: { id: "codex-1", label: "you@example.com" },
    currentAccount: true,
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 48, resetsAt: at(5 * 24 * 60 + 23 * 60), windowSeconds: 604_800, observedAt: OBSERVED },
    ],
    credits: { balance: "0", unlimited: false },
    subscription: { activeUntil: at(26 * 24 * 60), willRenew: true, checkedAt: at(-30) },
  },
  {
    provider: "codex",
    status: "ok",
    account: { id: "codex-2", label: "other@example.com" },
    currentAccount: false,
    savedProfile: true,
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 97, resetsAt: at(-85), windowSeconds: 604_800, observedAt: REMEMBERED_OBSERVED },
    ],
    subscription: { activeUntil: at(3 * 24 * 60 + 12 * 60), willRenew: false, note: "cancelled", checkedAt: at(-26 * 60) },
  },
];

const MUSE: ProviderLimits[] = [
  {
    provider: "muse",
    status: "ok",
    account: { id: "muse-1", label: "you@example.com" },
    currentAccount: true,
    plan: "wonder",
    windows: [
      { id: "weekly", label: "Weekly · all models", kind: "weekly", usedPercent: 22, resetsAt: at(6 * 24 * 60 + 4 * 60), observedAt: OBSERVED },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 41, resetsAt: at(2 * 60 + 15), windowSeconds: 18_000, observedAt: OBSERVED },
    ],
  },
];

function museIdle(): ProviderLimits[] {
  return [{
    ...MUSE[0],
    plan: "Muse Code Power Usage",
    windows: [
      { id: "weekly", label: "Weekly · all models", kind: "weekly", usedPercent: 30, resetsAt: at(17 * 60 + 24), observedAt: at(-33 * 60) },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 0, resetsAt: null, windowSeconds: 18_000, observedAt: OBSERVED },
    ],
  }];
}

function subscriptionBadgesCodex(): ProviderLimits[] {
  const template = { ...CODEX[0], windows: CODEX[0].windows.filter(window => window.kind === "weekly"), credits: null };
  const cases: Array<[string, ProviderLimits["subscription"]]> = [
    ["renewal", { activeUntil: at(30 * 24 * 60), willRenew: true, checkedAt: at(-30) }],
    ["later", { activeUntil: at(14 * 24 * 60), willRenew: false, checkedAt: at(-30) }],
    ["halfway", { activeUntil: at(3.5 * 24 * 60), willRenew: false, note: "cancelled", checkedAt: at(-30) }],
    ["deadline", { activeUntil: at(60), willRenew: false, note: "cancelled", checkedAt: at(-30) }],
    ["passed", { activeUntil: at(-24 * 60), willRenew: false, note: "cancelled", checkedAt: at(-30) }],
    ["overdue-renewal", { activeUntil: at(-24 * 60), willRenew: true, note: "pastDue", checkedAt: at(-30) }],
    ["plan-change", { activeUntil: at(9 * 24 * 60), willRenew: true, note: "planChange", checkedAt: at(-30) }],
    ["token-only", null],
  ];
  return cases.map(([id, subscription], index) => ({
    ...template,
    account: { id: `badge:${id}`, label: `${id}@example.com` },
    currentAccount: index === 0,
    windows: template.windows.map(window => ({ ...window, usedPercent: 25 + index * 8 })),
    subscription,
  }));
}

function claudeWithoutReset(): ProviderLimits[] {
  return [CLAUDE[0], {
    ...CLAUDE[0],
    account: { id: "claude-2", label: "other@example.com" },
    currentAccount: false,
    savedProfile: true,
    windows: CLAUDE[0].windows.map(window => window.kind === "session"
      ? { ...window, usedPercent: 0, resetsAt: null }
      : window),
  }];
}

function claudeWithoutWeekly(): ProviderLimits[] {
  return [CLAUDE[0], {
    ...CLAUDE[0],
    account: { id: "claude-2", label: "other@example.com" },
    currentAccount: false,
    savedProfile: true,
    windows: CLAUDE[0].windows.filter(window => window.kind !== "weekly"),
  }];
}

const BAND = [50, 75, 85, 95];

function band(source: ProviderLimits): ProviderLimits[] {
  const { account, windows } = source;
  if (!account) return [];
  return BAND.map((percent) => ({
    ...source,
    account: { ...account, id: `band-${percent}`, label: `${percent}% of the week` },
    currentAccount: percent === BAND[0],
    credits: null,
    windows: windows.slice(0, 2).map((window) => ({ ...window, usedPercent: percent })),
  }));
}

function limitsOrderClaude(): ProviderLimits[] {
  const source = CLAUDE[0];
  const rung = (id: string, label: string, plan: string, weekly: number, session: number, weeklyResetsIn: number, sessionResetsIn: number, currentAccount = false): ProviderLimits => ({
    ...source,
    account: { id: `order-${id}`, label },
    currentAccount,
    plan,
    windows: source.windows.slice(0, 2).map((window) => window.kind === "weekly"
      ? { ...window, usedPercent: weekly, resetsAt: at(weeklyResetsIn) }
      : { ...window, usedPercent: session, resetsAt: at(sessionResetsIn) }),
  });
  return [
    rung("current", "you@example.com", "max ×5", 60, 45, 3 * 24 * 60, 2 * 60, true),
    rung("waiting", "waiting@example.com", "max ×5", 100, 100, 4 * 24 * 60, 2 * 60),
    rung("soon", "soon@example.com", "max ×5", 35, 100, 5 * 24 * 60, 40),
    rung("spare", "spare@example.com", "max ×5", 20, 10, 6 * 24 * 60, 4 * 60),
    rung("bigger", "bigger@example.com", "max ×20", 70, 60, 2 * 24 * 60, 60),
  ];
}

function limitsBandClaude(): ProviderLimits[] {
  return band(CLAUDE[0]);
}

function limitsBandCodex(): ProviderLimits[] {
  return band(CODEX[0]);
}

function bankedResetsCodex(): ProviderLimits[] {
  return CODEX.map((entry) => ({
    ...entry,
    resetCredits: entry.currentAccount
      ? {
          availableCount: 2,
          nextExpiresAt: at(11 * 24 * 60 + 19 * 60),
          resets: [
            { title: "Full reset", expiresAt: at(11 * 24 * 60 + 19 * 60) },
            { title: "Full reset", expiresAt: at(25 * 24 * 60 + 4 * 60) },
          ],
        }
      : { availableCount: 1, nextExpiresAt: null },
    windows: entry.currentAccount ? entry.windows.map((window) => ({ ...window, usedPercent: 100 })) : entry.windows,
    resetOffer: entry.currentAccount ? { price: { currency: "USD", amountMinorUnits: 800 } } : null,
  }));
}

function workspaceCreditsCodex(): ProviderLimits[] {
  return CODEX.map((entry) => ({
    ...entry,
    plan: "self_serve_business_prolite",
    credits: { balance: "0", unlimited: false },
    workspaceCredits: entry.currentAccount
      ? { limit: "25000", used: "8000", usedPercent: 32, resetsAt: at(6 * 24 * 60 + 11 * 60), reached: false }
      : { limit: "10000", used: "10000", usedPercent: 100, resetsAt: at(2 * 24 * 60), reached: true },
  }));
}

function creditsSpentCodex(): ProviderLimits[] {
  return CODEX.map((entry) => ({
    ...entry,
    plan: "self_serve_business_prolite",
    credits: { balance: "0", unlimited: false },
    creditsSpent: entry.currentAccount
      ? { last7Days: 18303.4, last30Days: 20299.7, updatedAt: at(-3 * 60) }
      : { last7Days: 0, last30Days: 412.5, updatedAt: at(-26 * 60) },
  }));
}

function sameEmailWorkspacesCodex(): ProviderLimits[] {
  return [["personal", "prolite"], ["business", "self_serve_business_prolite"]].map(([id, plan], index) => ({
    ...CODEX[0],
    account: { id: `profile:${id}`, label: "shared@example.com" },
    currentAccount: index === 0,
    savedProfile: index !== 0,
    plan,
  }));
}

function sameEmailWorkspacesAccounts(): AccountsReading {
  return {
    profiles: [["personal", "0d6c1f3e-5b2a-4c8e-9f10-2a7b3c4d5e61"], ["business", "7e9a2b4c-1d3f-4a5b-8c6d-9e0f1a2b3c47"]].map(([id, workspaceId], index) => ({
      id, observationId: `profile:${id}`, legacyId: workspaceId, identity: { provider: "codex", userId: "user-shared", workspaceId },
      label: "shared@example.com", email: "shared@example.com", category: null, savedAt: "2026-09-17T12:00:00Z", active: index === 0, needsLogin: false,
    })),
    nativeObservationId: "profile:personal", nativeAccount: null, recoveryRequired: false, notice: null,
  };
}

function savedRefreshPaused([live]: ProviderLimits[]): ProviderLimits[] {
  return [live, {
    ...live, currentAccount: false, savedProfile: true, status: "failed", account: { id: `${live.provider}-saved`, label: "other@example.com" },
    message: "Saved usage access has expired. The last reading is retained.",
  }];
}

const DUPLICATE_WORKSPACE = "ca292064-c3f4-453c-b15a-43ef63c46478";

function accountDuplicate(): LimitsScenario {
  let reconnected = false;
  return {
    codex: () => accountDuplicateCodex(reconnected),
    accounts: { codex: accountDuplicateAccounts },
    addAccount: () => { reconnected = true; },
  };
}

function accountDuplicateCodex(reconnected: boolean): ProviderLimits[] {
  const legacy = { ...CODEX[1], currentAccount: false, savedProfile: false, account: { id: DUPLICATE_WORKSPACE, label: "shared@example.com" } };
  return [CODEX[0], reconnected ? {
    ...legacy,
    account: { id: "profile:shared", legacyId: DUPLICATE_WORKSPACE, label: "shared@example.com" },
    savedProfile: true,
    windows: CODEX[0].windows.map(window => ({ ...window, usedPercent: 42 })),
    replaced: [legacy.account],
  } : legacy];
}

function accountDuplicateAccounts(): AccountsReading {
  return {
    profiles: [{ id: "saved", observationId: "profile:shared", legacyId: DUPLICATE_WORKSPACE, identity: { provider: "codex", userId: "shared-user", workspaceId: DUPLICATE_WORKSPACE }, label: "shared@example.com", email: "shared@example.com", savedAt: "2026-09-13T12:00:00Z", active: false, needsLogin: false }],
    nativeObservationId: "codex-1", nativeAccount: null, recoveryRequired: false, notice: null,
  };
}

const ARCHIVED_UNREAD: SavedProfile = {
  id: "unread", observationId: "profile:claude-unread", legacyId: "user-former", identity: { provider: "claude", userId: "user-former", workspaceId: "Former workspace" },
  label: "former@example.com", email: "former@example.com", category: "Old client", savedAt: "2026-07-01T09:00:00Z", active: false, needsLogin: false,
};

function archivedAccounts(): LimitsScenario {
  return {
    claude: () => [CLAUDE[0], {
      ...CLAUDE[0], account: { id: "claude-2", label: "person@acme.example" }, currentAccount: false, savedProfile: true, plan: "max ×5",
      windows: CLAUDE[0].windows.map(window => ({ ...window, usedPercent: window.usedPercent + 30 })),
    }],
    codex: () => [
      CODEX[0],
      { ...CODEX[1], subscription: { activeUntil: at(-2 * 24 * 60), willRenew: false, note: "cancelled", checkedAt: at(-30) } },
      { ...CODEX[1], account: { id: "codex-history", label: "old-team@example.com" }, savedProfile: false, plan: "plus", subscription: null },
    ],
    accounts: {
      claude: () => {
        const reading = savedAccounts("claude");
        return { ...reading, profiles: [...reading.profiles, ARCHIVED_UNREAD] };
      },
    },
    archived: { claude: ["claude-2", ARCHIVED_UNREAD.observationId], codex: ["codex-history"] },
  };
}

function savedAccounts(agent: AgentId): AccountsReading {
  if (agent === "muse") return savedMuseAccount();
  return {
    profiles: [
      { id: "personal", observationId: agent === "codex" ? "codex-1" : "claude-1", legacyId: agent === "codex" ? "Personal workspace" : "user-personal", identity: { provider: agent, userId: "user-personal", workspaceId: "Personal workspace" }, label: "personal@example.com", email: "personal@example.com", category: null, savedAt: "2026-08-24T18:30:00Z", active: true, needsLogin: false },
      { id: "work", observationId: agent === "codex" ? "codex-2" : "claude-2", legacyId: agent === "codex" ? "Acme workspace" : "user-work", identity: { provider: agent, userId: "user-work", workspaceId: "Acme workspace" }, label: "person@acme.example", email: "person@acme.example", category: "Client A / research", savedAt: "2026-08-23T14:20:00Z", active: false, needsLogin: false },
    ],
    nativeObservationId: agent === "codex" ? "codex-1" : "claude-1",
    nativeAccount: { provider: agent, userId: "user-personal", workspaceId: "Personal workspace" },
    recoveryRequired: false,
    notice: null,
  };
}

function savedMuseAccount(): AccountsReading {
  const identity = { provider: "muse" as const, userId: "you@example.com", workspaceId: "" };
  return {
    profiles: [{ id: "muse-you", observationId: "muse-1", legacyId: "email:you@example.com", identity, label: "you@example.com", email: "you@example.com", category: null, savedAt: "2026-10-16T18:00:00Z", active: true, needsLogin: false }],
    nativeObservationId: "muse-1",
    nativeAccount: identity,
    recoveryRequired: false,
    notice: null,
  };
}

function claudeSignInAccounts(): AccountsReading {
  const reading = savedAccounts("claude");
  const endsIn = (days: number) => new Date(Date.now() + days * 86_400_000).toISOString();
  const [personal, work] = reading.profiles;
  return { ...reading, profiles: [{ ...personal, signInEndsAt: endsIn(3) }, { ...work, pendingActivation: true, signInEndsAt: endsIn(-1) }] };
}

function claudeSignIn(): ProviderLimits[] {
  return [CLAUDE[0], {
    ...CLAUDE[0], account: { id: "claude-2", label: "person@acme.example" }, currentAccount: false, savedProfile: true, plan: "max ×5",
    status: "failed", message: "This account's saved login has ended. Sign in again.",
  }];
}

function paidThrough(accountId: string): SubscriptionDate {
  return {
    date: new Date(Date.now() + (accountId === "codex-2" ? 3 : 26) * 86_400_000).toISOString(),
    checkedAt: new Date(Date.now() - 8 * 86_400_000).toISOString(),
  };
}

export type LimitsScenario = Partial<Record<LimitsProvider, () => ProviderLimits[]>> & {
  accounts?: Partial<Record<LimitsProvider, () => AccountsReading>>;
  codexSubscription?: (accountId: string) => SubscriptionDate | null;
  addAccount?: () => void;
  archived?: Partial<Record<LimitsProvider, string[]>>;
};

export const LIMITS_SCENARIOS: Record<string, () => LimitsScenario> = {
  subscriptionMissing: () => ({ codexSubscription: () => null }),
  subscriptionBadges: () => ({ codex: subscriptionBadgesCodex }),
  claudeMissingReset: () => ({ claude: claudeWithoutReset }),
  claudeNoWeekly: () => ({ claude: claudeWithoutWeekly }),
  museIdle: () => ({ muse: museIdle }),
  claudeSignIn: () => ({ claude: claudeSignIn, accounts: { claude: claudeSignInAccounts } }),
  savedRefreshPaused: () => ({ claude: () => savedRefreshPaused(CLAUDE), codex: () => savedRefreshPaused(CODEX) }),
  limitsBand: () => ({ claude: limitsBandClaude, codex: limitsBandCodex }),
  limitsOrder: () => ({
    claude: limitsOrderClaude,
    accounts: { claude: () => ({ profiles: [], nativeObservationId: "order-current", nativeAccount: null, recoveryRequired: false, notice: null }) },
  }),
  bankedResets: () => ({ codex: bankedResetsCodex }),
  bankedResetsAuto: () => ({ codex: bankedResetsCodex }),
  bankedResetsEarly: () => ({
    codex: () => bankedResetsCodex().map((entry) => entry.currentAccount
      ? { ...entry, windows: entry.windows.map((window) => ({ ...window, usedPercent: 40 })), resetOffer: null }
      : entry),
  }),
  sameEmailWorkspaces: () => ({ codex: sameEmailWorkspacesCodex, accounts: { codex: sameEmailWorkspacesAccounts } }),
  workspaceCredits: () => ({ codex: workspaceCreditsCodex }),
  creditsSpent: () => ({ codex: creditsSpentCodex }),
  accountDuplicate,
  archivedAccounts,
};

const OK = { claude: () => CLAUDE, codex: () => CODEX, muse: () => MUSE };

export function limitsScenario(name: string) {
  const chosen: LimitsScenario = Object.hasOwn(LIMITS_SCENARIOS, name) ? LIMITS_SCENARIOS[name]() : {};
  const provider = (agent: unknown) => (isAgentId(agent) && hasLimits(agent) ? agent : null);
  const archived: Record<LimitsProvider, Set<string>> = { claude: new Set(chosen.archived?.claude), codex: new Set(chosen.archived?.codex), muse: new Set(chosen.archived?.muse) };
  const forgotten: Record<LimitsProvider, Set<string>> = { claude: new Set(), codex: new Set(), muse: new Set() };
  const removedLogins: Record<LimitsProvider, Set<string>> = { claude: new Set(), codex: new Set(), muse: new Set() };
  const categories: Record<LimitsProvider, Map<string, string | null>> = { claude: new Map(), codex: new Map(), muse: new Map() };
  const accountsOf = (which: LimitsProvider) => {
    const answer = chosen.accounts?.[which];
    return answer ? answer() : savedAccounts(which);
  };
  const profilesOf = (which: LimitsProvider): SavedProfile[] => !hasSavedAccounts(which) ? []
    : accountsOf(which).profiles.flatMap(profile => removedLogins[which].has(profile.id) ? []
      : [archived[which].has(profile.observationId) ? { ...profile, archived: true } : profile]);
  const categoriesOf = (which: LimitsProvider): Record<string, string> => {
    const saved = profilesOf(which).flatMap(profile => profile.category ? [[profile.observationId, profile.category] as const] : []);
    const merged = new Map<string, string | null>([...saved, ...categories[which]]);
    return Object.fromEntries([...merged].flatMap(([id, category]) => category ? [[id, category]] : []));
  };
  const asColumn = (which: LimitsProvider, read: ProviderLimits[]): ProviderLimits[] => {
    const profiles = profilesOf(which);
    const named = categoriesOf(which);
    const facts = (id: string | undefined) => ({
      ...(profiles.find(profile => profile.observationId === id) ? { profile: profiles.find(profile => profile.observationId === id) } : {}),
      ...(id && named[id] ? { category: named[id] } : {}),
    });
    const unread = profiles.filter(profile => !read.some(entry => entry.account?.id === profile.observationId)).map((profile): ProviderLimits => ({
      provider: which, status: "ok", account: { id: profile.observationId, legacyId: profile.legacyId, label: profile.email },
      currentAccount: false, windows: [], unread: true, archived: !!profile.archived && !profile.active, ...facts(profile.observationId),
    }));
    return [...read.map(entry => ({ ...entry, ...facts(entry.account?.id) })), ...unread];
  };
  return {
    readLimits(agentId: unknown): ProviderLimits[] {
      const which = provider(agentId);
      if (!which) return [];
      return asColumn(which, (chosen[which] ?? OK[which])().flatMap(entry => {
        const id = entry.currentAccount ? null : entry.account?.id;
        if (!id) return [entry];
        if (forgotten[which].has(id)) return [];
        return [archived[which].has(id) ? { ...entry, archived: true } : entry];
      }));
    },
    readAccounts(agent: unknown): AccountsReading {
      const which = provider(agent);
      const reading = which ? accountsOf(which) : savedAccounts(agent as AgentId);
      return which ? { ...reading, profiles: profilesOf(which) } : reading;
    },
    readCategories(): AccountCategories {
      return Object.fromEntries(LIMITS_PROVIDERS.map(which => [which, categoriesOf(which)]));
    },
    setCategory(agentId: unknown, accountId: unknown, category: unknown): string | null {
      const which = provider(agentId);
      if (!which || typeof accountId !== "string") return null;
      const text = typeof category === "string" ? category.trim() : "";
      categories[which].set(accountId, text || null);
      return text || null;
    },
    readCodexSubscription(accountId: unknown): SubscriptionDate | null {
      return (chosen.codexSubscription ?? paidThrough)(String(accountId));
    },
    addAccount() {
      chosen.addAccount?.();
    },
    setArchived(agentId: unknown, ids: unknown, value: unknown) {
      const which = provider(agentId);
      if (!which || !Array.isArray(ids)) return;
      for (const id of ids.map(String)) {
        if (value === true) archived[which].add(id);
        else archived[which].delete(id);
      }
    },
    forgetSnapshot(agentId: unknown, accountId: unknown) {
      const which = provider(agentId);
      if (!which || typeof accountId !== "string") return;
      forgotten[which].add(accountId);
      archived[which].delete(accountId);
    },
    removeLogin(agent: unknown, profileId: unknown) {
      const which = provider(agent);
      if (which && typeof profileId === "string") removedLogins[which].add(profileId);
    },
  };
}
