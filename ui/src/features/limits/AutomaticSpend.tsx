import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { formatClock, formatResetIn } from "$lib/limitsFormat";
import { pendingResetSpendsQuery, rereadPendingResetSpends } from "$lib/sharedReads/resetSpends";
import type { PendingResetSpend } from "$lib/types";
import { actionButton } from "@/components/actionButton";

export function usePendingResetSpend(accountId: string | null): PendingResetSpend | null {
  const spends = useQuery({ ...pendingResetSpendsQuery(), enabled: accountId !== null });
  return spends.data?.find(waiting => waiting.accountId === accountId) ?? null;
}

export function AutomaticSpend({ accountId, dueAt, now }: { accountId: string; dueAt: string | null; now: number }) {
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

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

  const within = dueAt ? formatResetIn(dueAt, now) : "";
  return (
    <>
      {dueAt ? (
        <>
          <span className="text-[11px] text-[var(--mute)]">
            Using a banked reset at {formatClock(dueAt)}{within ? ` · in ${within}` : ""}
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
