import type { AccountsReading } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { SubscriptionDate } from "$lib/subscriptionTypes";
import type { AgentId } from "$lib/types";

/**
 * Synthetic subscription limits for the UI harness, dated against the harness clock
 * (`FIXTURE_CLOCK` in scripts/ui-shots.mjs): one live account per provider, plus a remembered
 * Codex account whose windows have both reset since it was last observed. `LIMITS_SCENARIOS` holds
 * every `?mock=<scenario>` that changes what the Limits commands answer.
 */

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
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 97, resetsAt: at(-85), windowSeconds: 604_800, observedAt: REMEMBERED_OBSERVED },
    ],
    subscription: { activeUntil: at(3 * 24 * 60 + 12 * 60), willRenew: false, note: "cancelled", checkedAt: at(-26 * 60) },
  },
];

/**
 * `?mock=subscriptionBadges`: one Codex card per state of the term badge, from a plan that renews
 * to one whose end has passed, with each note, and one card the endpoint never answered for, which
 * has only the login token's date.
 */
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

/** A saved Claude account whose five-hour session has not started. */
function claudeWithoutReset(): ProviderLimits[] {
  return [CLAUDE[0], {
    ...CLAUDE[0],
    account: { id: "claude-2", label: "other@example.com" },
    currentAccount: false,
    windows: CLAUDE[0].windows.map(window => window.kind === "session"
      ? { ...window, usedPercent: 0, resetsAt: null }
      : window),
  }];
}

/** A saved Claude account whose read reported no weekly window: its card leads with nothing. */
function claudeWithoutWeekly(): ProviderLimits[] {
  return [CLAUDE[0], {
    ...CLAUDE[0],
    account: { id: "claude-2", label: "other@example.com" },
    currentAccount: false,
    windows: CLAUDE[0].windows.filter(window => window.kind !== "weekly"),
  }];
}

/**
 * `?mock=limitsBand`: one account per rung of the usage ramp, for both providers.
 *
 * The ordinary fixtures all sit well below 70 %, so the band where a meter hardens toward red is
 * invisible in a capture — which is how an amber step that made a filling meter go paler survived
 * review. These windows walk 50 / 75 / 85 / 95 % so the whole ramp is on screen at once.
 *
 * Codex is here as well as Claude because its accent is the one that flips with the theme
 * (`var(--silkscreen)`: near-white on dark, near-black on light), so it is the arm that shows what
 * the ramp does from each end. Claude's `#d97757` is the same literal in both themes.
 */
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

/**
 * `?mock=limitsOrder`: five Claude accounts listed in the order the backend hands them over, which
 * the screen must not keep. The active login comes first whatever its usage; then the two with
 * usage left, the Max ×20 at 30% ahead of the untouched-looking Max ×5 because its percentage is
 * worth four times as much; then the two that are out of usage, the one usable again in forty
 * minutes ahead of the one waiting four days for its weekly window.
 */
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

/**
 * `?mock=bankedResets`: the signed-in Codex account has two banked resets and plenty of usage left,
 * so spending one asks first; the remembered account reports the one it had when last seen.
 */
function bankedResetsCodex(): ProviderLimits[] {
  return CODEX.map((entry) => ({
    ...entry,
    resetCredits: entry.currentAccount
      ? { availableCount: 2, nextExpiresAt: at(11 * 24 * 60 + 19 * 60) }
      : { availableCount: 1, nextExpiresAt: null },
    // Codex offers a paid reset only while an account sits at its limit, so the live card is at 100%.
    windows: entry.currentAccount ? entry.windows.map((window) => ({ ...window, usedPercent: 100 })) : entry.windows,
    resetOffer: entry.currentAccount ? { price: { currency: "USD", amountMinorUnits: 800 } } : null,
  }));
}

/**
 * `?mock=workspaceCredits` on Codex: two business workspace members. The live one has used part of
 * its share of the workspace's credits; the remembered one has used all of it. Both own balances
 * read 0, as they do in a workspace.
 */
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

/**
 * `?mock=creditsSpent` on Codex: two business workspace members without a per-member cap, so Codex
 * reports no share, only what each spent. Their own balances read 0, which the card leaves out.
 */
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

/**
 * `?mock=claudeSubscriptionStatus`: the signed-in Claude account behind on payment and a remembered
 * one whose subscription was canceled, as the profile's `organization.subscription_status` says.
 */
function claudeSubscriptionStatusClaude(): ProviderLimits[] {
  return [
    { ...CLAUDE[0], subscriptionStatus: "past_due" },
    { ...CLAUDE[0], account: { id: "claude-2", label: "team@example.com" }, currentAccount: false, subscriptionStatus: "canceled" },
  ];
}

/** `?mock=bankedResets` on Claude: one saved reset, reported with where Claude Code spends it. */
function bankedResetsClaude(): ProviderLimits[] {
  return CLAUDE.map((entry) => ({ ...entry, resetCredits: { availableCount: 1, nextExpiresAt: at(13 * 24 * 60 + 4 * 60) } }));
}

/**
 * `?mock=sameEmailWorkspaces`: one email signed in to a personal and a business workspace, which
 * the cards tell apart by plan.
 */
function sameEmailWorkspacesCodex(): ProviderLimits[] {
  return [["personal", "prolite"], ["business", "self_serve_business_prolite"]].map(([id, plan], index) => ({
    ...CODEX[0],
    account: { id: `profile:${id}`, label: "shared@example.com" },
    currentAccount: index === 0,
    plan,
  }));
}

/** `?mock=sameEmailWorkspaces`: both workspaces saved under the one email, the personal one in use. */
function sameEmailWorkspacesAccounts(): AccountsReading {
  return {
    profiles: [["personal", "0d6c1f3e-5b2a-4c8e-9f10-2a7b3c4d5e61"], ["business", "7e9a2b4c-1d3f-4a5b-8c6d-9e0f1a2b3c47"]].map(([id, workspaceId], index) => ({
      id, observationId: `profile:${id}`, identity: { provider: "codex", userId: "user-shared", workspaceId },
      label: "shared@example.com", email: "shared@example.com", category: null, savedAt: "2026-09-17T12:00:00Z", active: index === 0, needsLogin: false,
    })),
    nativeObservationId: "profile:personal", nativeAccount: null, recoveryRequired: false, notice: null,
  };
}

/**
 * `?mock=savedRefreshPaused`: the live account, and a saved one whose last read failed, so the card
 * keeps its last reading.
 */
function savedRefreshPaused([live]: ProviderLimits[]): ProviderLimits[] {
  return [live, {
    ...live, currentAccount: false, status: "failed", account: { id: `${live.provider}-saved`, label: "other@example.com" },
    message: "Saved usage access has expired. The last reading is retained.",
  }];
}

/**
 * `?mock=accountDuplicate`: a saved Codex login whose history an older app version wrote under its
 * workspace id, without the legacyId that ties it to the scoped reading. Signing in again
 * (`reconnectDuplicate`) brings the scoped reading, which the card then merges the history into.
 */
const DUPLICATE_WORKSPACE = "ca292064-c3f4-453c-b15a-43ef63c46478";
let duplicateReconnected = false;

export function reconnectDuplicate() {
  duplicateReconnected = true;
}

function accountDuplicateCodex(): ProviderLimits[] {
  const legacy = { ...CODEX[1], currentAccount: false, account: { id: DUPLICATE_WORKSPACE, label: "shared@example.com" } };
  // Older app versions can rewrite the scoped snapshot without its optional legacyId.
  return [CODEX[0], ...(duplicateReconnected ? [{ ...legacy,
    account: { id: "profile:shared", label: "shared@example.com" },
    windows: CODEX[0].windows.map(window => ({ ...window, usedPercent: 42 })),
  }] : []), legacy];
}

function accountDuplicateAccounts(): AccountsReading {
  return {
    profiles: [{ id: "saved", observationId: "profile:shared", identity: { provider: "codex", userId: "shared-user", workspaceId: DUPLICATE_WORKSPACE }, label: "shared@example.com", email: "shared@example.com", savedAt: "2026-09-13T12:00:00Z", active: false, needsLogin: false }],
    nativeObservationId: "codex-1", nativeAccount: null, recoveryRequired: false, notice: null,
  };
}

/** The saved accounts every scenario has unless it says otherwise: the live one, and one more. */
function savedAccounts(agent: AgentId): AccountsReading {
  return {
    profiles: [
      { id: "personal", observationId: agent === "codex" ? "codex-1" : "claude-1", identity: { provider: agent, userId: "user-personal", workspaceId: "Personal workspace" }, label: "personal@example.com", email: "personal@example.com", category: null, savedAt: "2026-08-24T18:30:00Z", active: true, needsLogin: false },
      { id: "work", observationId: agent === "codex" ? "codex-2" : "claude-2", identity: { provider: agent, userId: "user-work", workspaceId: "Acme workspace" }, label: "person@acme.example", email: "person@acme.example", category: "Client A / research", savedAt: "2026-08-23T14:20:00Z", active: false, needsLogin: false },
    ],
    nativeObservationId: agent === "codex" ? "codex-1" : "claude-1",
    nativeAccount: { provider: agent, userId: "user-personal", workspaceId: "Personal workspace" },
    recoveryRequired: false,
    notice: null,
  };
}

/** Relative to the real clock: the badge hides a date that has passed, whatever the harness clock says. */
function paidThrough(accountId: string): SubscriptionDate {
  return {
    date: new Date(Date.now() + (accountId === "codex-2" ? 3 : 26) * 86_400_000).toISOString(),
    checkedAt: new Date(Date.now() - 8 * 86_400_000).toISOString(),
  };
}

type LimitsProvider = Extract<AgentId, "claude" | "codex">;

/** What one scenario makes the Limits commands answer. Whatever it leaves out answers as `ok` does. */
export type LimitsScenario = Partial<Record<LimitsProvider, () => ProviderLimits[]>> & {
  accounts?: Partial<Record<LimitsProvider, () => AccountsReading>>;
  codexSubscription?: (accountId: string) => SubscriptionDate | null;
};

export const LIMITS_SCENARIOS: Record<string, LimitsScenario> = {
  subscriptionMissing: { codexSubscription: () => null },
  subscriptionBadges: { codex: subscriptionBadgesCodex },
  claudeMissingReset: { claude: claudeWithoutReset },
  claudeNoWeekly: { claude: claudeWithoutWeekly },
  savedRefreshPaused: { claude: () => savedRefreshPaused(CLAUDE), codex: () => savedRefreshPaused(CODEX) },
  limitsBand: { claude: limitsBandClaude, codex: limitsBandCodex },
  limitsOrder: {
    claude: limitsOrderClaude,
    accounts: { claude: () => ({ profiles: [], nativeObservationId: "order-current", nativeAccount: null, recoveryRequired: false, notice: null }) },
  },
  bankedResets: { claude: bankedResetsClaude, codex: bankedResetsCodex },
  sameEmailWorkspaces: { codex: sameEmailWorkspacesCodex, accounts: { codex: sameEmailWorkspacesAccounts } },
  workspaceCredits: { codex: workspaceCreditsCodex },
  creditsSpent: { codex: creditsSpentCodex },
  claudeSubscriptionStatus: { claude: claudeSubscriptionStatusClaude },
  accountDuplicate: { codex: accountDuplicateCodex, accounts: { codex: accountDuplicateAccounts } },
};

const OK = { claude: () => CLAUDE, codex: () => CODEX };

/** The Limits commands' answers under `?mock=<name>`; a name that is not a Limits scenario answers as `ok`. */
export function limitsScenario(name: string) {
  const chosen: LimitsScenario = Object.hasOwn(LIMITS_SCENARIOS, name) ? LIMITS_SCENARIOS[name] : {};
  const provider = (agent: unknown) => (agent === "claude" || agent === "codex" ? agent : null);
  return {
    readLimits(agentId: unknown): ProviderLimits[] {
      const which = provider(agentId);
      return which ? (chosen[which] ?? OK[which])() : [];
    },
    readAccounts(agent: unknown): AccountsReading {
      const which = provider(agent);
      const answer = which ? chosen.accounts?.[which] : undefined;
      return answer ? answer() : savedAccounts(agent as AgentId);
    },
    readCodexSubscription(accountId: unknown): SubscriptionDate | null {
      return (chosen.codexSubscription ?? paidThrough)(String(accountId));
    },
  };
}
