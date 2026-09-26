import { useEffect, useLayoutEffect, useState } from "react";
import { useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { UsageStatusBadge } from "./UsageStatusBadge";
import { RefreshCw } from "lucide-react";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import { usageFillStyle } from "$lib/limitsFormat";
import type { ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import { applyStoredTheme } from "$lib/theme";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { limitCards, type CardWindow, type LimitCard } from "./limitCards";
import { limitsRefreshMs, useLimitsProviders } from "./useLimitsProviders";

export function LimitsPopover() {
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<string | null>(null);
  const settings = useQuery({
    queryKey: ["app-settings"],
    queryFn: api.loadAppSettings,
    staleTime: Infinity,
  });
  const pollMinutes = settings.data?.limitsPollMinutes ?? DEFAULT_APP_SETTINGS.limitsPollMinutes;
  const { providers, loading, now, refresh } = useLimitsProviders(pollMinutes);
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
        void settings.refetch().then(({ data }) => {
          const currentPollMinutes = data?.limitsPollMinutes ?? DEFAULT_APP_SETTINGS.limitsPollMinutes;
          return queryClient.refetchQueries({
            queryKey: ["limits"],
            type: "active",
            predicate: (query) =>
              query.state.dataUpdatedAt > 0 &&
              Date.now() - query.state.dataUpdatedAt >=
                limitsRefreshMs(currentPollMinutes),
          });
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
  }, [queryClient, settings.refetch]);

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
          <h1 className="m-0 text-[15px] leading-tight font-semibold tracking-[-0.01em]">Limits</h1>
          <p className="mt-0.5 mb-0 text-[11px] text-[var(--mute)] tabular-nums">
            {loading ? "Updating…" : "Subscription usage"}
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
  // No saved profiles: reading them opens the vault and the native store, which the popover never does.
  const cards = limitCards({ provider, entries: query.data, profiles: [], now });
  const error = query.error ? displayError(parseInvokeError(query.error), name) : null;
  const errorBanner = error ? (
    <p
      className={`m-0 px-3 py-2 text-[12px] text-[var(--trip)] ${cards ? "border-b border-[var(--popover-hair)]" : ""}`}
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
        <h2 className="m-0 text-[12px] font-semibold tracking-[0.045em] uppercase">{name}</h2>
        {cards ? (
          <span className="ml-auto text-[11px] text-[var(--mute)]">
            {cards.length} {cards.length === 1 ? "account" : "accounts"}
          </span>
        ) : null}
      </div>

      <div className="overflow-hidden rounded-[11px] border border-[var(--popover-hair)] bg-[var(--popover-card)]">
        {errorBanner}
        {!cards ? (
          <p className="m-0 px-3 py-3 text-[12px] text-[var(--mute)]">
            {query.isFetching ? "Checking limits…" : "No data yet."}
          </p>
        ) : cards.length === 0 ? (
          <p className="m-0 px-3 py-3 text-[12px] text-[var(--mute)]">No saved accounts.</p>
        ) : (
          cards.map((card, index) => (
            <PopoverAccount key={card.key} card={card} divided={index > 0} />
          ))
        )}
      </div>
    </section>
  );
}

function PopoverAccount({ card, divided }: { card: LimitCard; divided: boolean }) {
  const { provider, reading, status, freshness } = card;
  const name = providerLabel(provider);
  const account = reading?.account;
  const label = account?.label ?? name;
  const windows = card.headline ? [card.headline, ...card.rows] : card.rows;

  return (
    <article
      aria-label={`${name} limits${account?.label ? ` · ${account.label}` : ""}`}
      className={`${divided ? "border-t border-[var(--popover-hair)]" : ""} px-2.5 py-2`}
      data-current-account={card.active ? "true" : "false"}
      data-status={freshness.readStatus}
    >
      <header className="mb-1.5 min-w-0">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{label}</span>
          {status?.kind === "savedRefresh" ? <UsageStatusBadge detail={status.detail} /> : status?.kind === "remembered" ? (
            <span className="shrink-0 rounded-full bg-[var(--popover-control)] px-1.5 py-0.5 type-badge text-[var(--mute)] uppercase">
              Remembered account
            </span>
          ) : status?.kind === "paused" ? (
            <span className="shrink-0 rounded-full bg-[var(--popover-control)] px-1.5 py-0.5 type-badge text-[var(--mute)] uppercase">
              Refresh paused
            </span>
          ) : null}
          {card.plan ? <span className="shrink-0 type-badge text-[var(--mute)] uppercase">{card.plan}</span> : null}
        </div>
        {freshness.updatedAt ? <p className="mt-0.5 mb-0 text-[10px] text-[var(--mute)] tabular-nums">Latest observation {freshness.updatedAt}</p> : null}
      </header>

      {freshness.message ? (
        <p className={`m-0 text-[12px] ${freshness.failed ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}>
          {freshness.message}
        </p>
      ) : null}

      {windows.length > 0 ? (
        <div className={`flex flex-col gap-1.5 ${freshness.message ? "mt-1.5" : ""}`}>
          {windows.map((window) => (
            <PopoverWindow key={window.id} window={window} provider={provider} />
          ))}
        </div>
      ) : card.empty ? (
        <p className="m-0 text-[12px] text-[var(--mute)]">No rate-limit windows.</p>
      ) : null}
    </article>
  );
}

function PopoverWindow({ window, provider }: { window: CardWindow; provider: AgentId }) {
  const { label, percent, note, text, color } = window;

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-2 gap-y-1">
      <div className="min-w-0">
        <span className="block truncate text-[11px] font-semibold tracking-[0.025em] text-[var(--mute)] uppercase">
          {label}
        </span>
        {note ? <span className="mt-0.5 block text-[10px] leading-tight text-[var(--mute)] tabular-nums">{note}</span> : null}
      </div>
      <span className="text-[13px] font-semibold tabular-nums" style={{ color }}>
        {text}
      </span>
      <div
        className="col-span-2 h-1 overflow-hidden rounded-full bg-[var(--popover-track)]"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(percent)}
      >
        <div
          className="h-full rounded-full transition-[width]"
          style={{ width: `${percent}%`, ...usageFillStyle(provider, percent) }}
        />
      </div>
    </div>
  );
}
