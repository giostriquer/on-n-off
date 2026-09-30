import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { SettingRow, SettingsCard, cardButton, rowLabel } from "@/components/SettingsCard";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import type { UsageHistoryStatus } from "$lib/usageTypes";

const HISTORY_KEY = ["usage-history"];

const DAY = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" });

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
    <SettingsCard
      label="Usage history"
      title="Usage history"
      description="Claude Code deletes transcripts after 30 days unless told otherwise. Once usage is a week old, on-n-off keeps its numbers, never the conversations, so Usage still counts it after the transcript is gone. A transcript deleted sooner than that is not kept."
    >
      <SettingRow>
        <div className={rowLabel} aria-live="polite">
          {current
            ? describe(current)
            : status.error && `Could not read the usage history: ${parseInvokeError(status.error).message}`}
        </div>
        {clearable && !confirming && (
          <button type="button" className={cardButton} onClick={() => setConfirming(true)}>
            Clear history
          </button>
        )}
      </SettingRow>
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
              className={cardButton}
              disabled={clear.isPending}
              onClick={() => clear.mutate()}
            >
              Confirm clear
            </button>
            <button type="button" className={cardButton} onClick={() => setConfirming(false)}>
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
    </SettingsCard>
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
