import { queryOptions, type QueryClient, type QueryKey } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { SharedReadSource } from "$lib/types";

const RESET_SPENDS = ["reset-spends"] as const;

export const resetSpendsAnnouncements = {
  "limits:reset-spends": [RESET_SPENDS],
} satisfies Record<Extract<SharedReadSource, "limits:reset-spends">, readonly QueryKey[]>;

export function pendingResetSpendsQuery() {
  return queryOptions({ queryKey: RESET_SPENDS, queryFn: () => api.pendingResetSpends() });
}

export function rereadPendingResetSpends(client: QueryClient): Promise<void> {
  return client.invalidateQueries({ queryKey: RESET_SPENDS });
}
