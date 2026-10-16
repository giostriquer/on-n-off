import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { formatClock, formatResetIn } from "$lib/limitsFormat";
import { pendingResetSpendsQuery, rereadPendingResetSpends } from "$lib/sharedReads/resetSpends";
import { actionButton } from "@/components/actionButton";

export function AutomaticSpend({ accountId, now }: { accountId: string; now: number }) {
  const client = useQueryClient();
  const spends = useQuery(pendingResetSpendsQuery());
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const spend = spends.data?.find(waiting => waiting.accountId === accountId);

  async function cancel() {
    setBusy(true);
    setProblem(null);
    try {
      if (!(await api.cancelResetSpend(accountId))) setProblem("It was already being used, or no longer waiting.");
      await rereadPendingResetSpends(client);
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
          <button type="button" className={actionButton} disabled={busy} aria-label="Cancel the automatic banked reset" onClick={() => void cancel()}>
            Cancel
          </button>
        </>
      ) : null}
      {problem ? <p role="alert" className="m-0 basis-full text-[11px] text-[var(--trip)]">{problem}</p> : null}
    </>
  );
}
