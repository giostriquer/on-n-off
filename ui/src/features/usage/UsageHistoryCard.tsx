import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import type { UsageHistoryStatus } from "$lib/usageTypes";

const HISTORY_KEY = ["usage-history"];

const DAY = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" });

const BUTTON =
  "h-8 rounded-md border border-[var(--hair)] px-2.5 text-[10px] font-semibold tracking-[0.04em] uppercase disabled:opacity-45";

/**
 * The usage on-n-off keeps after the agents delete their transcripts (`usage/history.rs`): how far
 * back it reaches, and a way to forget it. Clearing loses for good whatever only the history
 * still holds, so it asks first.
 */
export function UsageHistoryCard() {
  const client = useQueryClient();
  const status = useQuery({ queryKey: HISTORY_KEY, queryFn: () => api.usageHistoryStatus() });
  const [confirming, setConfirming] = useState(false);
  const clear = useMutation({
    mutationFn: () => api.clearUsageHistory(),
    onSuccess: (next) => {
      client.setQueryData(HISTORY_KEY, next);
      setConfirming(false);
      void client.invalidateQueries({ queryKey: ["usage"] });
    },
  });
  const current = status.data;
  const clearable = current !== undefined && current.state !== "empty";

  return (
    <section
      aria-label="Usage history"
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
    >
      <div className="px-3.5 py-3">
        <h3 className="m-0 text-[13px] font-semibold">Usage history</h3>
        <p className="mt-1 mb-0 text-[12px] text-[var(--mute)]">
          Claude Code deletes transcripts after 30 days unless told otherwise. on-n-off keeps the
          numbers of usage older than a week, never the conversations, so Usage still counts it once
          the transcript is gone.
        </p>
      </div>
      <div className="flex flex-wrap items-center gap-3 border-t border-[var(--hair)] px-3.5 py-2.5">
        <div className="min-w-0 flex-1 text-[12px] text-[var(--mute)]" aria-live="polite">
          {current
            ? describe(current)
            : status.error && `Could not read the usage history: ${parseInvokeError(status.error).message}`}
        </div>
        {clearable && !confirming && (
          <button type="button" className={BUTTON} onClick={() => setConfirming(true)}>
            Clear history
          </button>
        )}
      </div>
      {confirming && (
        <div
          role="group"
          aria-label="Confirm clearing usage history"
          className="flex flex-col gap-2 border-t border-[var(--hair)] px-3.5 py-2.5 text-[12px]"
        >
          <p className="m-0">
            Clear the kept usage? Usage whose transcripts are already deleted is lost for good; usage
            still in a transcript is counted again from it.
          </p>
          <div className="flex gap-2">
            <button
              type="button"
              className={BUTTON}
              disabled={clear.isPending}
              onClick={() => clear.mutate()}
            >
              Confirm clear
            </button>
            <button type="button" className={BUTTON} onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {clear.error && (
        <p role="alert" className="m-0 px-3.5 pb-2.5 text-[12px] text-[var(--trip)]">
          {parseInvokeError(clear.error).message}
        </p>
      )}
    </section>
  );
}

function describe(status: UsageHistoryStatus): string {
  switch (status.state) {
    case "empty":
      return "Nothing kept yet. Usage is kept once it is a week old.";
    case "unreadable":
      return "The history file could not be read, so Usage counts transcripts alone. on-n-off leaves the file as it is until it is cleared.";
    case "kept": {
      const since = status.keptSince ? `Kept since ${DAY.format(new Date(status.keptSince))}` : "Kept";
      return `${since} · ${formatBytes(status.bytes)}`;
    }
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
