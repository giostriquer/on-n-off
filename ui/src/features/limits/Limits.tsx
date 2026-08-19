import { useState } from "react";
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
  usageTone,
  type UsageTone,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import { providerColor } from "$lib/providerStyle";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { useLimitsProviders } from "./useLimitsProviders";

/** Percent text picks up the warning tone; the bar itself stays in the provider colour. */
const TONE_COLOR: Record<UsageTone, string | undefined> = {
  calm: undefined,
  warn: "var(--warn)",
  trip: "var(--trip)",
};

export function Limits() {
  // Only Claude and Codex carry a subscription the backend can read; the rest report `unsupported`.
  const { providers, loading, asOf, now, refresh } = useLimitsProviders();

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]" data-testid="limits-screen" aria-busy={loading}>
      <header className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1 font-mono text-[12px] text-[var(--mute)]">
          Subscription limits
          {asOf ? ` · as of ${formatClock(asOf)}` : null}
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
        read from the endpoints the CLIs use, with the login each CLI already stores · on-n-off never writes
        to or refreshes those logins · accounts you have signed out of stay listed with the numbers from
        their last read
      </p>
    </div>
  );
}

/** The live card for a provider plus one card per remembered account. */
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
          current?.filter((entry) => entry.live || entry.account?.id !== accountId),
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
          key={`${entry.live ? "live" : "kept"}-${entry.account?.id ?? index}`}
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
function CardHeader({ entry, provider }: { entry?: ProviderLimits; provider: AgentId }) {
  const name = providerLabel(provider);
  const label = entry?.account?.label ?? null;
  const plan = planLabel(entry?.plan);
  const credits = entry?.credits ?? null;
  return (
    <header className="flex items-center gap-2.5 border-b border-[var(--hair)] px-3.5 py-2.5">
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
  const stale = !entry.live;
  const [hero, ...rest] = entry.windows;

  return (
    <section
      className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={title}
      data-status={entry.status}
      data-live={entry.live ? "true" : "false"}
    >
      <CardHeader entry={entry} provider={entry.provider} />

      {stale ? (
        <div className="flex items-center gap-2.5 border-b border-[var(--hair)] bg-[var(--well)] px-3.5 py-[7px]">
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--mute)]">
            as of {formatResetAt(entry.fetchedAt)}
            {label ? ` · sign in as ${label} to refresh` : " · sign in again to refresh"}
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

      {entry.status !== "ok" ? (
        <p className={`px-3.5 py-4 text-[13px] ${entry.status === "failed" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}>
          {entry.message ?? `${name} limits are unavailable.`}
        </p>
      ) : !hero ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{name} reported no rate-limit windows.</p>
      ) : (
        <>
          <HeroWindow window={hero} provider={entry.provider} now={now} stale={stale} />
          {rest.map((window) => (
            <WindowRow key={window.id} window={window} provider={entry.provider} now={now} stale={stale} />
          ))}
        </>
      )}
    </section>
  );
}

/** What one window says right now; a remembered window past its reset says nothing usable. */
function readWindow(window: LimitWindow, now: number, stale: boolean) {
  const resetSince = stale && hasElapsed(window.resetsAt, now);
  const percent = resetSince ? 0 : window.usedPercent;
  const tone = usageTone(percent);
  const resetAt = formatResetAt(window.resetsAt);
  const resetIn = formatResetIn(window.resetsAt, now);
  const note = resetSince
    ? `reset since ${resetAt || "then"} · sign in to refresh`
    : resetIn
      ? `resets in ${resetIn}${resetAt ? ` · ${resetAt}` : ""}`
      : "";
  return { percent, tone, note, text: resetSince ? "—" : formatUsedPercent(percent) };
}

function Meter({
  window,
  percent,
  tone,
  provider,
  className,
}: {
  window: LimitWindow;
  percent: number;
  tone: UsageTone;
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
      data-tone={tone}
    >
      <div className="h-full rounded-sm transition-[width]" style={{ width: `${percent}%`, background: providerColor(provider) }} />
    </div>
  );
}

/** The first (weekly) window as the card's headline, in the Overview's big-number idiom. */
function HeroWindow({ window, provider, now, stale }: { window: LimitWindow; provider: AgentId; now: number; stale: boolean }) {
  const { percent, tone, note, text } = readWindow(window, now, stale);
  return (
    <div className="flex flex-col gap-2 p-3.5">
      <span className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{window.label}</span>
      <div className="flex items-end gap-2">
        <span className="text-[34px] leading-none font-semibold tracking-[-0.03em]" style={{ color: TONE_COLOR[tone] }}>
          {text}
        </span>
        {note ? <span className="min-w-0 truncate pb-1 font-mono text-[11.5px] text-[var(--mute)]">{note}</span> : null}
      </div>
      <Meter window={window} percent={percent} tone={tone} provider={provider} className="h-1.5" />
    </div>
  );
}

/**
 * Remaining windows as compact rows: small-caps label with its reset note underneath (never
 * truncated), bar and percent on the right — the same idiom as the Overview's list rows.
 */
function WindowRow({ window, provider, now, stale }: { window: LimitWindow; provider: AgentId; now: number; stale: boolean }) {
  const { percent, tone, note, text } = readWindow(window, now, stale);
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
      <Meter window={window} percent={percent} tone={tone} provider={provider} className="h-1 w-24 shrink-0" />
      <span className="w-11 shrink-0 text-right font-mono text-[12px]" style={{ color: TONE_COLOR[tone] }}>
        {text}
      </span>
    </div>
  );
}
