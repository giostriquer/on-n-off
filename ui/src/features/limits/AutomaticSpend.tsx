import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { formatClock, formatResetIn } from "$lib/limitsFormat";
import { usePendingResetSpends } from "$lib/usePendingResetSpends";
import { PENDING_RESET_SPENDS_KEY } from "$lib/useSharedRead";
import { accountButton } from "@/features/accounts/AccountManager";

/**
 * The banked reset an automatic alert is waiting to use on this account (`limits_monitor::
 * auto_spend`): when, and a Cancel that keeps it. The monitor uses it then unless it is cancelled
 * here, and says in a notification what came of it.
 */
export function AutomaticSpend({ accountId, now }: { accountId: string; now: number }) {
  const client = useQueryClient();
  const spends = usePendingResetSpends();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const spend = spends.data?.find(waiting => waiting.accountId === accountId);

  async function cancel() {
    setBusy(true);
    setProblem(null);
    try {
      if (!(await api.cancelResetSpend(accountId))) setProblem("It was already being used, or no longer waiting.");
      await client.invalidateQueries({ queryKey: PENDING_RESET_SPENDS_KEY });
    } catch (error) {
      setProblem(parseInvokeError(error).message);
    } finally {
      setBusy(false);
    }
  }

  const within = spend ? formatResetIn(spend.dueAt, now) : "";
  return (
    <>
      {spend ? (
        <>
          <span className="text-[11px] text-[var(--mute)]">
            Using a banked reset at {formatClock(spend.dueAt)}{within ? ` · in ${within}` : ""}
          </span>
          <button type="button" className={accountButton} disabled={busy} aria-label="Cancel the automatic banked reset" onClick={() => void cancel()}>
            Cancel
          </button>
        </>
      ) : null}
      {problem ? <p role="alert" className="m-0 basis-full text-[11px] text-[var(--trip)]">{problem}</p> : null}
    </>
  );
}
