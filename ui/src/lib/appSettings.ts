import type { AgentId, AppSettings, GithubPollSeconds, LimitsPollMinutes, ResetAlert } from "./types";

export const ALL_AGENTS: readonly AgentId[] = ["claude", "codex", "antigravity", "cursor"];

export const DEFAULT_APP_SETTINGS: AppSettings = {
  hiddenAgents: [],
  binaryPaths: {},
  automaticUpdates: true,
  limitNotifications: false,
  limitsPollMinutes: 5,
  githubScopes: [],
  githubNotifications: false,
  githubPollSeconds: 60,
  closeToTray: false,
  resetAlerts: {},
};

/** The share of the current limit left at or under which Codex's own app lets a reset be used. */
export const CODEX_RESET_MAX_LEFT_PERCENT = 10;

/** The longest wait an alert can ask for before the limit renews by itself: a week, the cycle. */
export const RESET_ALERT_MAX_HOURS = 7 * 24;

/** A new alert's defaults: Codex's own share, and a day before the limit renews by itself. */
export function defaultResetAlert(label: string | null): ResetAlert {
  return { label, maxLeftPercent: CODEX_RESET_MAX_LEFT_PERCENT, minHoursToRenewal: 24, automatic: false };
}

/**
 * The share of the current limit left at or under which `accountId`'s banked reset may be spent:
 * Codex's own 10%, or the lower share its alert names. The backend keeps the same rule.
 */
export function resetSpendLimit(alerts: Record<string, ResetAlert>, accountId: string): number {
  const alert = alerts[accountId];
  return alert ? Math.min(alert.maxLeftPercent, CODEX_RESET_MAX_LEFT_PERCENT) : CODEX_RESET_MAX_LEFT_PERCENT;
}

/** `alerts` with `accountId`'s alert set to `alert`, or removed when it is `null`. */
export function withResetAlert(alerts: Record<string, ResetAlert>, accountId: string, alert: ResetAlert | null): Record<string, ResetAlert> {
  const next = { ...alerts };
  if (alert) next[accountId] = alert;
  else delete next[accountId];
  return next;
}

export function mergeAppSettings(overlay: Partial<AppSettings> | null | undefined): AppSettings {
  return {
    hiddenAgents: overlay?.hiddenAgents ?? [],
    binaryPaths: overlay?.binaryPaths ?? {},
    automaticUpdates: overlay?.automaticUpdates ?? true,
    limitNotifications: overlay?.limitNotifications ?? false,
    limitsPollMinutes: normalizeLimitsPollMinutes(overlay?.limitsPollMinutes),
    githubScopes: overlay?.githubScopes ?? [],
    githubNotifications: overlay?.githubNotifications ?? false,
    githubPollSeconds: normalizeGithubPollSeconds(overlay?.githubPollSeconds),
    closeToTray: overlay?.closeToTray ?? false,
    resetAlerts: overlay?.resetAlerts ?? {},
  };
}

function normalizeLimitsPollMinutes(value: number | undefined): LimitsPollMinutes {
  return value === 5 || value === 10 || value === 15 || value === 30 ? value : 5;
}

function normalizeGithubPollSeconds(value: number | undefined): GithubPollSeconds {
  return value === 30 || value === 60 || value === 120 || value === 300 ? value : 60;
}

export function visibleAgentIds(hidden: readonly AgentId[]): AgentId[] {
  const set = new Set(hidden);
  const visible = ALL_AGENTS.filter((id) => !set.has(id));
  return visible.length > 0 ? [...visible] : ["claude"];
}

/** Returns the next hidden list. Refuses to hide the last visible provider. */
export function setAgentHidden(hidden: readonly AgentId[], id: AgentId, hide: boolean): AgentId[] {
  const next = new Set(hidden);
  if (hide) {
    next.add(id);
    if (ALL_AGENTS.every((agent) => next.has(agent))) {
      return ALL_AGENTS.filter((agent) => hidden.includes(agent));
    }
  } else {
    next.delete(id);
  }
  return ALL_AGENTS.filter((agent) => next.has(agent));
}
