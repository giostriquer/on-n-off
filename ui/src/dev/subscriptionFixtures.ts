import type { SubscriptionReading } from "$lib/subscriptionTypes";
import { limitsFor } from "./limitsFixtures";

const NOW = Date.parse("2026-08-24T20:00:00Z");
const DAY = 86_400_000;
const cases = [
  {id: "renewal", days: 30, kind: "renews"},
  {id: "later", days: 14, kind: "expires"},
  {id: "halfway", days: 3.5, kind: "expires"},
  {id: "deadline", days: 1 / 24, kind: "expires"},
  {id: "passed", days: -1, kind: "expires"},
  {id: "unavailable", days: 2, kind: "expires", unavailable: true},
  {id: "overdue-renewal", days: -1, kind: "renews"},
  {id: "unknown", days: 14, kind: "paidThrough"},
] as const;

export function subscriptionBadgeLimits() {
  const template = limitsFor("codex")[0];
  return cases.map((item, index) => ({
    ...template, account: {id: `badge:${item.id}`, label: `${item.id}@example.com`}, currentAccount: index === 0, credits: null,
    windows: template.windows.filter(window => window.kind === "weekly").map(window => ({...window, usedPercent: 25 + index * 8})),
  }));
}

export function subscriptionBadgeProfiles() {
  return cases.map((item, index) => ({
    id: `badge:${item.id}`, observationId: `badge:${item.id}`,
    identity: {provider: "codex", userId: item.id, workspaceId: item.id},
    label: `${item.id}@example.com`, email: `${item.id}@example.com`, category: null,
    savedAt: new Date(NOW).toISOString(), active: index === 0, needsLogin: false,
  }));
}

export function subscriptionBadgeReading(accountId: unknown): SubscriptionReading {
  const item = cases.find(item => `badge:${item.id}` === accountId);
  if (!item) return {metadata: null, connected: false, unavailable: false};
  const unavailable = "unavailable" in item;
  return {
    metadata: {date: new Date(NOW + item.days * DAY).toISOString(), kind: item.kind,
      source: item.kind === "paidThrough" ? "localToken" : "billing",
      checkedAt: new Date(NOW - (unavailable ? 2 * DAY : 0)).toISOString(), stale: unavailable},
    connected: false, unavailable, browserSupported: true, canConnect: true,
  };
}
