import { useQuery } from "@tanstack/react-query";
import * as api from "$lib/api";
import { PENDING_RESET_SPENDS_KEY, useSharedRead } from "$lib/useSharedRead";

export { PENDING_RESET_SPENDS_KEY };

/**
 * The banked resets automatic alerts are waiting to use. The monitor schedules and spends them and
 * the cards cancel them, each announcing the change as the `limits:reset-spends` shared read.
 */
export function usePendingResetSpends() {
  useSharedRead("limits:reset-spends");
  return useQuery({ queryKey: PENDING_RESET_SPENDS_KEY, queryFn: api.pendingResetSpends });
}
