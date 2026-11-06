import { queryOptions, type QueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { readForced } from "$lib/sharedReads/forcedRead";
import { makeWindow, type UsageWindow } from "$lib/usageFormat";
import type { UsageHistoryStatus, UsageSummary } from "$lib/usageTypes";
import { USAGE_QUERY_ROOT } from "./usageKeys";

const FOLDED_USAGE_STATUS = ["usage-history"] as const;

export type UsageQueryResult = {
  summary: UsageSummary;
  window: UsageWindow;
};

function usageWindowKey(days: number) {
  return [...USAGE_QUERY_ROOT, days] as const;
}

async function loadUsageWindow(days: number, force: boolean): Promise<UsageQueryResult> {
  const window = makeWindow(days);
  const summary = await api.usageSummary({
    sinceDay: window.sinceDay,
    untilDay: window.untilDay,
    timeZone: window.timeZone,
    resolution: window.resolution,
    sinceTime: window.sinceTime,
    untilTime: window.untilTime,
    force,
  });
  return { summary, window };
}

export function usageWindowQuery(days: number, onRead?: () => void) {
  return queryOptions({
    queryKey: usageWindowKey(days),
    queryFn: () => {
      onRead?.();
      return loadUsageWindow(days, false);
    },
    refetchOnWindowFocus: true,
  });
}

export async function refreshUsageWindow(client: QueryClient, days: number): Promise<void> {
  await readForced(client, usageWindowKey(days), (force) => loadUsageWindow(days, force)).catch(() => undefined);
}

export function foldedUsageStatusQuery() {
  return queryOptions({ queryKey: FOLDED_USAGE_STATUS, queryFn: () => api.usageHistoryStatus() });
}

export async function clearFoldedUsage(client: QueryClient): Promise<UsageHistoryStatus> {
  const status = await api.clearUsageHistory();
  client.setQueryData(foldedUsageStatusQuery().queryKey, status);
  void client.invalidateQueries({ queryKey: USAGE_QUERY_ROOT });
  return status;
}
