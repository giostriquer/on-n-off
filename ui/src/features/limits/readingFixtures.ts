import type { LimitsStatus, ProviderLimits, ReadStanding, StandingMessage } from "$lib/limitsTypes";
import type { AgentId } from "$lib/types";

export const NOW = "2026-08-17T20:00:00Z";

export const REMEMBERED: ReadStanding = { kind: "lastObserved", why: "remembered" };

export function lastObserved(why: "readFailed" | "signedOut" | "savedRefreshPaused", text: string, tone: StandingMessage["tone"] = why === "signedOut" ? "muted" : "error"): ReadStanding {
  return { kind: "lastObserved", why, message: { text, tone } };
}

export function unavailable(text: string, tone: StandingMessage["tone"] = "muted"): ReadStanding {
  return { kind: "unavailable", message: { text, tone } };
}

export function okClaude(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "claude",
    status: "ok",
    account: { id: "uuid-1", label: "me@claude.example" },
    currentAccount: true,
    plan: "max",
    planLabel: "Max",
    planCapacity: 5,
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
    planLabel: "Pro ×20",
    planCapacity: 20,
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 74, resetsAt: "2026-08-24T23:34:33Z", observedAt: NOW },
      { id: "extra:gpt-5.6-luna", label: "Weekly · GPT-5.6-Luna", kind: "model", usedPercent: 3, resetsAt: "2026-08-17T19:59:00Z", observedAt: NOW },
    ],
    credits: { balance: "12.5", unlimited: false },
    ...overrides,
  };
}

export function okMuse(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "muse",
    status: "ok",
    account: { id: "email:you@muse.example", label: "you@muse.example" },
    currentAccount: true,
    plan: "wonder",
    planLabel: "Wonder",
    windows: [
      { id: "weekly", label: "Weekly · all models", kind: "weekly", usedPercent: 22, resetsAt: "2026-08-24T20:00:00Z", observedAt: NOW },
      { id: "session", label: "5 hour · all models", kind: "session", usedPercent: 41, resetsAt: "2026-08-17T22:15:00Z", windowSeconds: 18_000, observedAt: NOW },
    ],
    ...overrides,
  };
}

export function staleCodex(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "codex",
    status: "ok",
    account: { id: "acct-personal", label: "personal@codex.example" },
    currentAccount: false,
    standing: REMEMBERED,
    plan: "plus",
    planLabel: "Plus",
    windows: [
      { id: "primary", label: "Weekly · all models", kind: "weekly", usedPercent: 88, resetsAt: "2026-08-20T10:00:00Z", observedAt: "2026-08-16T21:40:00.000Z" },
      { id: "secondary", label: "5 hour · all models", kind: "session", usedPercent: 40, resetsAt: "2026-08-16T22:00:00Z", observedAt: "2026-08-16T21:40:00.000Z" },
    ],
    ...overrides,
  };
}

export function statusOnly(provider: AgentId, status: LimitsStatus, message: string, tone: StandingMessage["tone"] = "muted"): ProviderLimits {
  return { provider, status, message, currentAccount: true, windows: [], standing: unavailable(message, tone) };
}
