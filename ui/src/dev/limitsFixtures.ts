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
    plan: "max",
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
  },
  {
    provider: "codex",
    status: "ok",
    account: { id: "codex-2", label: "other@example.com" },
    currentAccount: false,
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 97, resetsAt: at(-85), windowSeconds: 604_800, observedAt: REMEMBERED_OBSERVED },
      { id: "extra:spark", label: "5 hour · GPT-5.3-Codex-Spark", kind: "model", usedPercent: 0, resetsAt: at(-4 * 24 * 60 - 7 * 60), observedAt: at(-4 * 24 * 60 - 17 * 60) },
    ],
  },
];

const LIMITS: Partial<Record<AgentId, ProviderLimits[]>> = { claude: CLAUDE, codex: CODEX };

export function limitsFor(agentId: unknown): ProviderLimits[] {
  return (typeof agentId === "string" && LIMITS[agentId as AgentId]) || [];
}
