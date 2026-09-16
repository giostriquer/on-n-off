import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";

/**
 * Synthetic subscription limits for the UI harness, dated against the harness clock
 * (`FIXTURE_CLOCK` in scripts/ui-shots.mjs): one live account per provider, plus a remembered
 * Codex account whose windows have both reset since it was last observed.
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
      { id: "extra:spark", label: "5 hour · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 0, resetsAt: at(5 * 60), windowSeconds: 18_000, observedAt: OBSERVED },
      { id: "extra:spark:secondary", label: "Weekly · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 0, resetsAt: at(7 * 24 * 60), windowSeconds: 604_800, observedAt: OBSERVED },
    ],
    credits: { balance: "0", unlimited: false },
  },
  {
    provider: "codex",
    status: "ok",
    account: { id: "codex-2", label: "other@example.com" },
    currentAccount: false,
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 97, resetsAt: at(-85), windowSeconds: 604_800, observedAt: REMEMBERED_OBSERVED },
      { id: "extra:spark", label: "5 hour · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 0, resetsAt: at(-4 * 24 * 60 - 7 * 60), windowSeconds: 18_000, observedAt: REMEMBERED_OBSERVED },
    ],
  },
];

const LIMITS: Partial<Record<AgentId, ProviderLimits[]>> = { claude: CLAUDE, codex: CODEX };

/** A saved Claude account whose five-hour session has not started. */
export function claudeWithoutReset(): ProviderLimits[] {
  return [CLAUDE[0], {
    ...CLAUDE[0],
    account: { id: "claude-2", label: "other@example.com" },
    currentAccount: false,
    windows: CLAUDE[0].windows.map(window => window.kind === "session"
      ? { ...window, usedPercent: 0, resetsAt: null }
      : window),
  }];
}

export function limitsFor(agentId: unknown): ProviderLimits[] {
  return (typeof agentId === "string" && LIMITS[agentId as AgentId]) || [];
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

export function limitsBandClaude(): ProviderLimits[] {
  return band(CLAUDE[0]);
}

export function limitsBandCodex(): ProviderLimits[] {
  return band(CODEX[0]);
}

/**
 * `?mock=bankedResets`: the signed-in Codex account has two banked resets and plenty of usage left,
 * so spending one asks first; the remembered account reports the one it had when last seen.
 */
export function bankedResetsCodex(): ProviderLimits[] {
  return CODEX.map((entry) => ({
    ...entry,
    resetCredits: entry.currentAccount
      ? { availableCount: 2, nextExpiresAt: at(11 * 24 * 60 + 19 * 60) }
      : { availableCount: 1, nextExpiresAt: null },
  }));
}

