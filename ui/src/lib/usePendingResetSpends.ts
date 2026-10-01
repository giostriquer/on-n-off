import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { PENDING_RESET_SPENDS_KEY, useSharedRead } from "$lib/useSharedRead";

export function usePendingResetSpends() {
  useSharedRead("limits:reset-spends");
  return useQuery({ queryKey: PENDING_RESET_SPENDS_KEY, queryFn: api.pendingResetSpends });
}
