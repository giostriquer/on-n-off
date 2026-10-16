import { PROVIDER_IDS } from "./providers";
import type { AgentId, AppSettings, ResetAlert } from "./types";

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

export const CODEX_RESET_MAX_LEFT_PERCENT = 5;

export const RESET_ALERT_MIN_LEFT_PERCENT = 1;

export const RESET_AUTO_SPEND_DELAY_MINUTES = 10;

export const RESET_ALERT_MIN_HOURS = 0;

export const RESET_ALERT_MAX_HOURS = 7 * 24;

export function defaultResetAlert(label: string | null): ResetAlert {
  return { label, maxLeftPercent: CODEX_RESET_MAX_LEFT_PERCENT, minHoursToRenewal: 24, automatic: false };
}

export function resetSpendLimit(alerts: Record<string, ResetAlert>, accountId: string): number {
  const alert = alerts[accountId];
  return alert ? Math.min(alert.maxLeftPercent, CODEX_RESET_MAX_LEFT_PERCENT) : CODEX_RESET_MAX_LEFT_PERCENT;
}

export function withResetAlert(alerts: Record<string, ResetAlert>, accountId: string, alert: ResetAlert | null): Record<string, ResetAlert> {
  const next = { ...alerts };
  if (alert) next[accountId] = alert;
  else delete next[accountId];
  return next;
}

export function withBinaryPath(
  paths: AppSettings["binaryPaths"],
  id: AgentId,
  path: string,
): AppSettings["binaryPaths"] {
  const next = { ...paths };
  if (path.trim()) next[id] = path.trim();
  else delete next[id];
  return next;
}

export function visibleAgentIds(hidden: readonly AgentId[]): AgentId[] {
  const set = new Set(hidden);
  const visible = PROVIDER_IDS.filter((id) => !set.has(id));
  return visible.length > 0 ? visible : PROVIDER_IDS.slice(0, 1);
}

export function setAgentHidden(hidden: readonly AgentId[], id: AgentId, hide: boolean): AgentId[] {
  const next = new Set(hidden);
  if (hide) {
    next.add(id);
    if (PROVIDER_IDS.every((agent) => next.has(agent))) {
      return PROVIDER_IDS.filter((agent) => hidden.includes(agent));
    }
  } else {
    next.delete(id);
  }
  return PROVIDER_IDS.filter((agent) => next.has(agent));
}
