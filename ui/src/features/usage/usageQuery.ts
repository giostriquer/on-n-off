import { queryOptions } from "@tanstack/react-query";
import * as api from "$lib/api";
import { makeWindow, type UsageWindow } from "$lib/usageFormat";
import type { UsageSummary } from "$lib/usageTypes";
import { USAGE_QUERY_ROOT } from "./usageFocusReads";

export type UsageQueryResult = {
  summary: UsageSummary;
  window: UsageWindow;
};

export async function loadUsageWindow(days: number, force = false): Promise<UsageQueryResult> {
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

export function usageWindowQuery(days: number, revision: number, read: () => Promise<UsageQueryResult>) {
  return queryOptions({
    queryKey: [...USAGE_QUERY_ROOT, days, revision] as const,
    queryFn: read,
    refetchOnWindowFocus: true,
  });
}
