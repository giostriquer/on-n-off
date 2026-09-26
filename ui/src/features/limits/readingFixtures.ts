import type { LimitsStatus, ProviderLimits } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";

/**
 * Readings the Limits tests share: a signed-in Claude and Codex account, a remembered Codex account
 * and a read with only a status, all dated against `NOW`. Tests only.
 */
export const NOW = "2026-08-17T20:00:00Z";

export function okClaude(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "claude",
    status: "ok",
    account: { id: "uuid-1", label: "me@claude.example" },
    currentAccount: true,
    plan: "max",
    windows: [
      { id: "weekly_all", label: "Weekly · all models", kind: "weekly", usedPercent: 12, resetsAt: "2026-08-24T13:59:59Z", observedAt: NOW },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 7, resetsAt: "2026-08-18T04:59:59Z", observedAt: NOW },
      { id: "weekly_opus", label: "Weekly · Opus", kind: "model", usedPercent: 91.4, resetsAt: "2026-08-24T13:59:59Z", observedAt: NOW },
    ],
    ...overrides,
  };
}

export function okCodex(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "codex",
    status: "ok",
    account: { id: "acct-work", label: "work@codex.example" },
    currentAccount: true,
    plan: "pro",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 74, resetsAt: "2026-08-24T23:34:33Z", observedAt: NOW },
      { id: "extra:gpt-5.6-luna", label: "Weekly · GPT-5.6-Luna", kind: "model", usedPercent: 3, resetsAt: "2026-08-17T19:59:00Z", observedAt: NOW },
    ],
    credits: { balance: "12.5", unlimited: false },
    ...overrides,
  };
}

/** A remembered reading of the other Codex account: read yesterday, its session already reset. */
export function staleCodex(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "codex",
    status: "ok",
    account: { id: "acct-personal", label: "personal@codex.example" },
    currentAccount: false,
    plan: "plus",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 88, resetsAt: "2026-08-20T10:00:00Z", observedAt: "2026-08-16T21:40:00.000Z" },
      { id: "secondary", label: "5 hour · all models", kind: "session", usedPercent: 40, resetsAt: "2026-08-16T22:00:00Z", observedAt: "2026-08-16T21:40:00.000Z" },
    ],
    ...overrides,
  };
}

export function statusOnly(provider: AgentId, status: LimitsStatus, message: string | null): ProviderLimits {
  return { provider, status, message, currentAccount: true, windows: [] };
}
