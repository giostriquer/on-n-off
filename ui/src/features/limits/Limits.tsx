import { useState } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import {
  planLabel,
  usageFillColor,
  usageToneColor,
  type UsageTone,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { presentLimitAccount, presentLimitWindow, RESET_VALUE_TEXT } from "./limitPresentation";
import { useLimitsProviders } from "./useLimitsProviders";

export function Limits() {
  // Only Claude and Codex carry a subscription the backend can read; the rest report `unsupported`.
  const { providers, loading, now, refresh } = useLimitsProviders();

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]" data-testid="limits-screen" aria-busy={loading}>
      <header className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1 font-mono text-[12px] text-[var(--mute)]">
          Subscription limits
          {loading ? (
            <span className="ml-2" role="status" aria-live="polite">
              · Checking…
            </span>
          ) : null}
        </div>
        <button
          type="button"
          className="inline-flex size-7 items-center justify-center rounded-md border border-[var(--hair)] bg-transparent text-[var(--mute)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fill)] disabled:opacity-45"
          disabled={loading}
          aria-label="Refresh limits"
          onClick={refresh}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
        </button>
      </header>

      <div className="grid items-start gap-3 lg:grid-cols-2">
        {providers.map(({ provider, query }) => (
          <ProviderColumn key={provider} provider={provider} query={query} now={now} />
        ))}
      </div>

      <p className="font-mono text-[10.5px] leading-snug text-[var(--mute)]">
        quota observations come from read-only sources tied to each subscription account · each account
        shows its latest observation · on-n-off does not implement provider login flows ·
        signed-out accounts remain visible with their last trustworthy observations
      </p>
    </div>
  );
}

/** The current account card for a provider plus one card per remembered account. */
function ProviderColumn({
  provider,
  query,
  allowForget = true,
  now,
}: {
  provider: AgentId;
  query: UseQueryResult<ProviderLimits[]>;
  allowForget?: boolean;
  now: number;
}) {
  const queryClient = useQueryClient();
  const name = providerLabel(provider);
  const entries = query.data ?? null;
  const [forgetError, setForgetError] = useState<string | null>(null);
  const error = query.error ? displayError(parseInvokeError(query.error), name) : forgetError;

  function forget(accountId: string) {
    setForgetError(null);
    api
      .forgetLimitsSnapshot(provider, accountId)
      .then(() => {
        queryClient.setQueryData<ProviderLimits[]>(["limits", provider], (current) =>
          current?.filter((entry) => entry.currentAccount || entry.account?.id !== accountId),
        );
      })
      .catch((reason: unknown) => {
        setForgetError(`Could not forget that account: ${displayError(parseInvokeError(reason), name)}`);
      });
  }

  if (!entries) {
    return (
      <section
        className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
        aria-label={`${name} limits`}
        data-status="pending"
      >
        <CardHeader provider={provider} />
        {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{query.isFetching ? "Checking limits…" : "No data yet."}</p>
      </section>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {entries.map((entry, index) => (
        <AccountCard
          key={`${entry.currentAccount ? "current" : "remembered"}-${entry.account?.id ?? index}`}
          entry={entry}
          now={now}
          error={index === 0 ? error : null}
          onForget={allowForget ? forget : undefined}
        />
      ))}
    </div>
  );
}

/** Same header row as the Overview cards: uppercase title, mono meta, chips on the right. */
function CardHeader({ entry, provider, updatedAt }: { entry?: ProviderLimits; provider: AgentId; updatedAt?: string | null }) {
  const name = providerLabel(provider);
  const label = entry?.account?.label ?? null;
  const plan = planLabel(entry?.plan);
  const credits = entry?.credits ?? null;
  return (
    <header className="border-b border-[var(--hair)] px-3.5 py-2.5">
      <div className="flex items-center gap-2.5">
        <ProviderIcon provider={provider} className="size-3.5 shrink-0" />
        <span className="text-[11.5px] font-semibold tracking-[0.03em] uppercase">{name}</span>
        {label ? <span className="min-w-0 truncate font-mono text-[11px] text-[var(--mute)]">{label}</span> : null}
        <div className="flex-1" />
        {credits ? (
          <span className="font-mono text-[11px] text-[var(--mute)]">
            {credits.unlimited ? "unlimited credits" : `${credits.balance} credits`}
          </span>
        ) : null}
        {plan ? (
          <span className="rounded-md border border-[var(--hair)] px-1.5 py-0.5 font-mono text-[10px] uppercase">
            {plan}
          </span>
        ) : null}
      </div>
      {updatedAt ? <p className="mt-1.5 mb-0 font-mono text-[10.5px] text-[var(--mute)]">Latest observation {updatedAt}</p> : null}
    </header>
  );
}

function AccountCard({
  entry,
  now,
  error,
  onForget,
}: {
  entry: ProviderLimits;
  now: number;
  error: string | null;
  onForget?: (accountId: string) => void;
}) {
  const name = providerLabel(entry.provider);
  const account = entry.account ?? null;
  const label = account?.label ?? null;
  const title = label ? `${name} limits · ${label}` : `${name} limits`;
  const [hero, ...rest] = entry.windows;
  const { message, refreshPaused, remembered, updatedAt } = presentLimitAccount(entry, `${name} limits are unavailable.`);

  return (
    <section
      className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={title}
      data-status={entry.status}
      data-current-account={entry.currentAccount ? "true" : "false"}
    >
      <CardHeader entry={entry} provider={entry.provider} updatedAt={updatedAt} />

      {remembered && hero ? (
        <div className="flex items-center gap-2.5 border-b border-[var(--hair)] bg-[var(--well)] px-3.5 py-[7px]">
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--mute)]">
            Remembered account{label ? ` · sign in as ${label} to refresh` : " · sign in again to refresh"}
          </span>
          {account && onForget ? (
            <button
              type="button"
              className="shrink-0 border-0 bg-transparent p-0 text-[11px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase hover:text-[var(--silkscreen)]"
              aria-label={`Forget ${label ?? account.id}`}
              onClick={() => onForget(account.id)}
            >
              Forget
            </button>
          ) : null}
        </div>
      ) : null}

      {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}

      {refreshPaused ? (
        <p className="px-3.5 pt-3 text-[13px] font-medium text-[var(--silkscreen)]">Refresh paused.</p>
      ) : null}

      {message ? (
        <p
          className={`px-3.5 ${hero ? "pt-1.5" : "py-4"} text-[13px] ${entry.status === "failed" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}
        >
          {message}
        </p>
      ) : null}

      {hero ? (
        <>
          <HeroWindow window={hero} provider={entry.provider} now={now} />
          {rest.map((window) => (
            <WindowRow
              key={window.id}
              window={window}
              provider={entry.provider}
              now={now}
            />
          ))}
        </>
      ) : entry.status === "ok" ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{name} reported no rate-limit windows.</p>
      ) : null}
    </section>
  );
}

function Meter({
  window,
  percent,
  tone,
  unavailable,
  provider,
  className,
}: {
  window: LimitWindow;
  percent: number;
  tone: UsageTone;
  unavailable: boolean;
  provider: AgentId;
  className: string;
}) {
  return (
    <div
      className={`overflow-hidden rounded-sm bg-[var(--well)] ${className}`}
      role="meter"
      aria-label={window.label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(percent)}
      aria-valuetext={unavailable ? RESET_VALUE_TEXT : undefined}
      data-tone={tone}
    >
      <div
        className="h-full rounded-sm transition-[width]"
        style={{ width: `${percent}%`, background: usageFillColor(provider, tone) }}
      />
    </div>
  );
}

/** The first (weekly) window as the card's headline, in the Overview's big-number idiom. */
function HeroWindow({
  window,
  provider,
  now,
}: {
  window: LimitWindow;
  provider: AgentId;
  now: number;
}) {
  const { percent, tone, note, text, unavailable } = presentLimitWindow(window, now);
  return (
    <div className="flex flex-col gap-2 p-3.5">
      <span className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{window.label}</span>
      <div className="flex items-end gap-2">
        <span
          className="text-[34px] leading-none font-semibold tracking-[-0.03em]"
          style={{ color: unavailable ? "var(--mute)" : usageToneColor(tone) }}
        >
          {text}
        </span>
        {note ? (
          <span className="min-w-0 flex-1 pb-0.5 font-mono text-[11.5px] leading-snug text-[var(--mute)]">{note}</span>
        ) : null}
      </div>
      <Meter window={window} percent={percent} tone={tone} unavailable={unavailable} provider={provider} className="h-1.5" />
    </div>
  );
}

/**
 * Remaining windows as compact rows: small-caps label with its reset note underneath (never
 * truncated), bar and percent on the right — the same idiom as the Overview's list rows.
 */
function WindowRow({
  window,
  provider,
  now,
}: {
  window: LimitWindow;
  provider: AgentId;
  now: number;
}) {
  const { percent, tone, note, text, unavailable } = presentLimitWindow(window, now);
  const model = window.kind === "model";
  return (
    <div className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span
          className={`truncate text-[10px] font-semibold tracking-[0.03em] uppercase ${model ? "text-[var(--mute)]" : ""}`}
        >
          {window.label}
        </span>
        {note ? <span className="font-mono text-[11px] leading-snug text-[var(--mute)]">{note}</span> : null}
      </div>
      <Meter window={window} percent={percent} tone={tone} unavailable={unavailable} provider={provider} className="h-1 w-24 shrink-0" />
      <span
        className="w-11 shrink-0 text-right font-mono text-[12px]"
        style={{ color: unavailable ? "var(--mute)" : usageToneColor(tone) }}
      >
        {text}
      </span>
    </div>
  );
}
