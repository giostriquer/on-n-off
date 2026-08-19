import { useEffect, useLayoutEffect, useState } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import {
  formatClock,
  formatResetAt,
  formatResetIn,
  formatUsedPercent,
  hasElapsed,
  planLabel,
  usageFillColor,
  usageTone,
  usageToneColor,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import { applyStoredTheme } from "$lib/theme";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { LIMITS_STALE_MS, useLimitsProviders } from "./useLimitsProviders";

export function LimitsPopover() {
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<string | null>(null);
  const { providers, loading, asOf, now, refresh } = useLimitsProviders();
  const [claude, codex] = providers;

  useLayoutEffect(() => {
    applyStoredTheme();
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        void api.hideLimitsPopover();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api
      .onLimitsPopoverOpened(() => {
        applyStoredTheme();
        void queryClient.refetchQueries({
          queryKey: ["limits"],
          type: "active",
          predicate: (query) =>
            query.state.dataUpdatedAt > 0 && Date.now() - query.state.dataUpdatedAt >= LIMITS_STALE_MS,
        });
      })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);

  async function runAction(label: string, action: () => Promise<void>) {
    setActionError(null);
    try {
      await action();
    } catch (reason) {
      setActionError(`Could not ${label}: ${displayError(parseInvokeError(reason), "on-n-off")}`);
    }
  }

  return (
    <main className="limits-popover-shell flex h-full min-h-0 flex-col overflow-hidden rounded-[16px] border border-[var(--popover-hair)] text-[var(--silkscreen)]">
      <header className="flex shrink-0 items-center gap-3 border-b border-[var(--popover-hair)] px-3.5 py-2.5">
        <div className="min-w-0 flex-1">
          <h1 className="m-0 text-[16px] leading-tight font-semibold tracking-[-0.01em]">Limits</h1>
          <p className="mt-0.5 mb-0 text-[11px] text-[var(--mute)] tabular-nums">
            {loading ? "Updating…" : asOf ? `Updated ${formatClock(asOf)}` : "Subscription usage"}
          </p>
        </div>
        <button
          type="button"
          className="inline-flex size-7 shrink-0 items-center justify-center rounded-full border-0 bg-[var(--popover-control)] text-[var(--mute)] transition-colors hover:text-[var(--silkscreen)] disabled:opacity-45"
          disabled={loading}
          aria-label="Refresh limits"
          onClick={refresh}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
        </button>
      </header>
      <div className="limits-popover-scroll flex min-h-0 flex-1 flex-col gap-3 px-2.5 py-2.5">
        <PopoverProviderSection provider={claude.provider} query={claude.query} now={now} />
        <PopoverProviderSection provider={codex.provider} query={codex.query} now={now} />
      </div>
      {actionError ? (
        <p className="shrink-0 border-t border-[var(--popover-hair)] px-3 py-2 text-[11px] text-[var(--trip)]" role="alert">
          {actionError}
        </p>
      ) : null}
      <footer className="flex shrink-0 items-center justify-between border-t border-[var(--popover-hair)] px-3 py-2">
        <button
          type="button"
          className="rounded-md border-0 bg-transparent px-1.5 py-1 text-[12px] font-medium text-[var(--mute)] hover:bg-[var(--popover-control)] hover:text-[var(--silkscreen)]"
          onClick={() => void runAction("quit on-n-off", api.quitApp)}
        >
          Quit
        </button>
        <button
          type="button"
          className="rounded-md border-0 bg-[var(--fill)] px-2.5 py-1.5 text-[12px] font-semibold text-[var(--fill-ink)] shadow-sm"
          onClick={() => void runAction("open on-n-off", api.openLimitsWindow)}
        >
          Open on-n-off
        </button>
      </footer>
    </main>
  );
}

function PopoverProviderSection({
  provider,
  query,
  now,
}: {
  provider: AgentId;
  query: UseQueryResult<ProviderLimits[]>;
  now: number;
}) {
  const name = providerLabel(provider);
  const entries = query.data ?? null;
  const error = query.error ? displayError(parseInvokeError(query.error), name) : null;
  const errorBanner = error ? (
    <p
      className={`m-0 px-3 py-2 text-[12px] text-[var(--trip)] ${entries ? "border-b border-[var(--popover-hair)]" : ""}`}
      role="alert"
      aria-label={`${name} refresh error`}
    >
      {error}
    </p>
  ) : null;

  return (
    <section aria-label={`${name} accounts`} className="flex flex-col gap-1.5">
      <div className="flex items-center gap-1.5 px-1">
        <ProviderIcon provider={provider} className="size-3.5 shrink-0" title="" />
        <h2 className="m-0 text-[11.5px] font-semibold tracking-[0.045em] uppercase">{name}</h2>
        {entries ? (
          <span className="ml-auto text-[10.5px] text-[var(--mute)]">
            {entries.length} {entries.length === 1 ? "account" : "accounts"}
          </span>
        ) : null}
      </div>

      <div className="overflow-hidden rounded-[11px] border border-[var(--popover-hair)] bg-[var(--popover-card)]">
        {errorBanner}
        {!entries ? (
          <p className="m-0 px-3 py-3 text-[12px] text-[var(--mute)]">
            {query.isFetching ? "Checking limits…" : "No data yet."}
          </p>
        ) : entries.length === 0 ? (
          <p className="m-0 px-3 py-3 text-[12px] text-[var(--mute)]">No saved accounts.</p>
        ) : (
          entries.map((entry, index) => (
            <PopoverAccount
              key={`${entry.live ? "live" : "saved"}-${entry.account?.id ?? index}`}
              entry={entry}
              now={now}
              divided={index > 0}
            />
          ))
        )}
      </div>
    </section>
  );
}

function PopoverAccount({ entry, now, divided }: { entry: ProviderLimits; now: number; divided: boolean }) {
  const name = providerLabel(entry.provider);
  const label = entry.account?.label ?? name;
  const plan = planLabel(entry.plan);
  const stale = !entry.live;

  return (
    <article
      aria-label={`${name} limits${entry.account?.label ? ` · ${entry.account.label}` : ""}`}
      className={`${divided ? "border-t border-[var(--popover-hair)]" : ""} px-2.5 py-2`}
      data-live={entry.live ? "true" : "false"}
      data-status={entry.status}
    >
      <header className="mb-1.5 flex min-w-0 items-center gap-1.5">
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{label}</span>
        {stale ? (
          <span className="shrink-0 rounded-full bg-[var(--popover-control)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
            Saved snapshot
          </span>
        ) : null}
        {plan ? <span className="shrink-0 text-[10px] text-[var(--mute)] uppercase">{plan}</span> : null}
      </header>

      {entry.status !== "ok" ? (
        <p className={`m-0 text-[12px] ${entry.status === "failed" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}>
          {entry.message ?? `${name} limits are unavailable.`}
        </p>
      ) : entry.windows.length === 0 ? (
        <p className="m-0 text-[12px] text-[var(--mute)]">No rate-limit windows.</p>
      ) : (
        <div className="flex flex-col gap-1.5">
          {entry.windows.map((window) => (
            <PopoverWindow key={window.id} window={window} provider={entry.provider} now={now} stale={stale} />
          ))}
        </div>
      )}
    </article>
  );
}

function PopoverWindow({
  window,
  provider,
  now,
  stale,
}: {
  window: LimitWindow;
  provider: AgentId;
  now: number;
  stale: boolean;
}) {
  const resetSince = stale && hasElapsed(window.resetsAt, now);
  const percent = resetSince ? 0 : window.usedPercent;
  const tone = usageTone(percent);
  const resetAt = formatResetAt(window.resetsAt);
  const resetIn = formatResetIn(window.resetsAt, now);
  const note = resetSince ? "Expired" : resetIn ? `Resets in ${resetIn}` : resetAt || "No reset time";
  const text = resetSince ? "—" : formatUsedPercent(percent);

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-2 gap-y-1">
      <div className="flex min-w-0 items-baseline gap-1.5">
        <span className="truncate text-[10.5px] font-semibold tracking-[0.025em] text-[var(--mute)] uppercase">
          {window.label}
        </span>
        <span className="shrink-0 text-[10px] text-[var(--mute)] tabular-nums">{note}</span>
      </div>
      <span className="text-[13px] font-semibold tabular-nums" style={{ color: usageToneColor(tone) }}>
        {text}
      </span>
      <div
        className="col-span-2 h-1 overflow-hidden rounded-full bg-[var(--popover-track)]"
        role="meter"
        aria-label={window.label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(percent)}
      >
        <div
          className="h-full rounded-full transition-[width]"
          style={{ width: `${percent}%`, background: usageFillColor(provider, tone) }}
        />
      </div>
    </div>
  );
}
