import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { PENDING_RESET_SPENDS_KEY } from "$lib/useSharedRead";

export function usePendingResetSpends() {
  return useQuery({ queryKey: PENDING_RESET_SPENDS_KEY, queryFn: api.pendingResetSpends });
}
