import { AccountCardActions } from "@/features/accounts/AccountCardActions";
import type { SavedProfile } from "$lib/accountTypes";
import { AccountControllers, AccountManager, useAccountManagement } from "@/features/accounts/AccountManager";
import { useState, type ReactNode } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { AddAccount } from "@/features/accounts/AddAccount";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import { planLabel } from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { headlineWindow, presentLimitAccount, presentLimitWindow } from "./limitPresentation";
import { AccountSubscriptionBadge } from "./SubscriptionBadge";
import { useLimitsProviders } from "./useLimitsProviders";
import { accountCards, orderAccountCards } from "./accountCards";
import { BankedResetsRow, CLAUDE_RESET_HINT, ResetOfferRow } from "./BankedResets";
import { CodexAccountActions } from "./CodexAccountActions";
import { UsageStatusBadge } from "./UsageStatusBadge";
import { CreditsRows } from "./Credits";
import { Meter, MeterRow } from "./Meter";

export function Limits({ pollMinutes = 5 }: { pollMinutes?: LimitsPollMinutes }) {
  return <AccountControllers><LimitsContent pollMinutes={pollMinutes} /></AccountControllers>;
}

function LimitsContent({ pollMinutes }: { pollMinutes: LimitsPollMinutes }) {
  // Only Claude and Codex carry a subscription the backend can read; the rest report `unsupported`.
  const { providers, loading, now } = useLimitsProviders(pollMinutes);

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]" data-testid="limits-screen" aria-busy={loading}>
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="m-0 text-[15px] font-semibold tracking-[0.05em] uppercase">Subscription limits</h2>
          <p className="mt-1 mb-0 font-mono text-[12px] text-[var(--mute)]">
            every {pollMinutes} minutes
            {loading ? (
              <span role="status" aria-live="polite">
                {" "}
                · Checking…
              </span>
            ) : null}
          </p>
        </div>
        <AddAccount />
      </header>

      <div className="grid items-start gap-3 lg:grid-cols-2">
        {providers.map(({ provider, query }) => (
          <div key={provider} className="flex flex-col gap-3">
            {(provider === "claude" || provider === "codex") ? <AccountManager provider={provider}>
              <ProviderColumn provider={provider} query={query} now={now} />
            </AccountManager> : <ProviderColumn provider={provider} query={query} now={now} />}
          </div>
        ))}
      </div>

      <p className="font-mono text-[11px] leading-snug text-[var(--mute)]">
        Reset times apply to usage limits. Signed-out accounts show their last known usage.
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
  const manager = useAccountManagement();
  const profiles = manager?.query.data?.profiles ?? [];
  const cards = accountCards(query.data ?? [], profiles);
  const known = query.data || profiles.length ? cards.entries : null;
  if (known) for (const profile of profiles) {
    if (profile.observationId && !known.some(entry => entry.account?.id === profile.observationId)) {
      known.push({ provider, status: "ok", account: { id: profile.observationId, label: profile.email }, currentAccount: profile.active, windows: [], message: "Usage unavailable." });
    }
  }
  const entries = known ? orderAccountCards(known, now) : null;
  const [forgetError, setForgetError] = useState<string | null>(null);
  const error = query.error ? displayError(parseInvokeError(query.error), name) : forgetError;

  async function forget(accountId: string) {
    setForgetError(null);
    // Keep the verified association from the confirmed card even after its saved login is removed.
    // Delete legacy history first so a partial failure keeps the scoped observation available.
    const legacy = cards.legacyAccounts.get(accountId) ?? [];
    const ids = [...legacy.map(account => account.id), accountId];
    try {
      for (const account of legacy) await api.forgetLimitsSnapshot(provider, account.id, account.email);
      await api.forgetLimitsSnapshot(provider, accountId);
      queryClient.setQueryData<ProviderLimits[]>(["limits", provider], (current) =>
        current?.filter((entry) => entry.currentAccount || !entry.account || !ids.includes(entry.account.id)),
      );
    } catch (reason: unknown) {
      setForgetError(`Could not remove that account: ${displayError(parseInvokeError(reason), name)}`);
      throw reason;
    }
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
          profile={profiles.find(profile => profile.observationId === entry.account?.id)}
          now={now}
          error={index === 0 ? error : null}
          onForget={allowForget ? forget : undefined}
        />
      ))}
    </div>
  );
}

/** Account identity stays prominent; workspace ids are never displayed. */
function CardHeader({ entry, provider, updatedAt, subscription, profile, menu, activeWithoutHeadline, savedRefreshDetail }: { entry?: ProviderLimits; provider: AgentId; updatedAt?: string | null; subscription?: ReactNode; profile?: SavedProfile; menu?: ReactNode; activeWithoutHeadline?: boolean; savedRefreshDetail?: string | null }) {
  const name = providerLabel(provider);
  const label = profile?.email ?? entry?.account?.label ?? null;
  const plan = planLabel(entry?.plan, provider);
  return (
    <header className="border-b border-[var(--hair)] px-3.5 py-2.5" title={updatedAt ? `Usage last checked ${updatedAt}` : undefined}>
      <div className="flex items-center gap-2.5">
        <ProviderIcon provider={provider} className="size-3.5 shrink-0 translate-y-[0.5px]" />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-semibold" title={label ?? name}>{label ?? name}</div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
        {activeWithoutHeadline && <ActiveAccountDot />}
        {subscription}
        {savedRefreshDetail && <UsageStatusBadge detail={savedRefreshDetail} />}
        {plan ? (
          <span className="rounded-md border border-[var(--hair)] px-1.5 py-0.5 type-badge uppercase">
            {plan}
          </span>
        ) : null}
        </div>
        {menu}
      </div>
      {profile?.category && <div className="mt-0.5 break-words pl-6 text-[11px] text-[var(--mute)]">{profile.category}</div>}
      {updatedAt ? <p className="sr-only">Latest observation {updatedAt}</p> : null}
    </header>
  );
}

function AccountCard({
  entry,
  profile,
  now,
  error,
  onForget,
}: {
  entry: ProviderLimits;
  profile?: SavedProfile;
  now: number;
  error: string | null;
  onForget?: (accountId: string) => Promise<void>;
}) {
  const name = providerLabel(entry.provider);
  const account = entry.account ?? null;
  const label = profile?.email ?? account?.label ?? null;
  const title = label ? `${name} limits · ${label}` : `${name} limits`;
  const { headline, rest } = headlineWindow(entry);
  const active = !!account && (profile?.active ?? entry.currentAccount);
  const presentation = presentLimitAccount(entry, `${name} limits are unavailable.`);
  const { message, refreshPaused, updatedAt, savedRefreshDetail } = presentation;

  const subscription = <AccountSubscriptionBadge entry={entry} now={now} freshness={presentation} />;
  const header = (menu: ReactNode) => <CardHeader savedRefreshDetail={savedRefreshDetail} activeWithoutHeadline={active && !headline} menu={menu} entry={entry} provider={entry.provider} updatedAt={updatedAt} subscription={subscription} profile={profile} />;
  const content = <>
      {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}

      {refreshPaused ? (
        <p className="px-3.5 pt-3 text-[13px] font-medium text-[var(--silkscreen)]">Refresh paused.</p>
      ) : null}

      {message ? (
        <p
          className={`px-3.5 ${entry.windows.length > 0 ? "pt-1.5" : "py-4"} text-[13px] ${entry.status === "failed" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}
        >
          {message}
        </p>
      ) : null}

      {headline ? <HeadlineWindow active={active} window={headline} provider={entry.provider} now={now} /> : null}
      {rest.map((window) => (
        <WindowRow
          key={window.id}
          window={window}
          provider={entry.provider}
          now={now}
        />
      ))}
      {entry.windows.length === 0 && entry.status === "ok" && !message ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{profile ? "Usage unavailable." : `${name} reported no rate-limit windows.`}</p>
      ) : null}

      <CreditsRows entry={entry} now={now} />
      <BankedResetsRow resetCredits={entry.resetCredits} hint={entry.provider === "claude" && entry.currentAccount ? CLAUDE_RESET_HINT : undefined} now={now} />
      {entry.provider === "codex" ? <ResetOfferRow offer={entry.resetOffer} /> : null}
  </>;
  return (
    <section
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={title}
      data-status={entry.status}
      data-current-account={entry.currentAccount ? "true" : "false"}
    >
      {account ? <AccountCardActions accountId={account.id} label={label ?? account.id} current={profile?.active ?? entry.currentAccount} profile={profile} onForget={onForget} header={header}
        footer={entry.provider === "codex" ? state => <CodexAccountActions entry={entry} label={label ?? account.id} now={now} state={state} /> : undefined}>
        {content}
      </AccountCardActions> : <>{header(null)}{content}</>}
    </section>
  );
}

function ActiveAccountDot() {
  return <span role="img" aria-label="Active account" title="Active account"
    className="size-[7px] shrink-0 rounded-full bg-[var(--live)] shadow-[0_0_8px_var(--live)]" />;
}

/** The card's headline window, in the Overview's big-number idiom. */
function HeadlineWindow({
  active,
  window,
  provider,
  now,
}: {
  active: boolean;
  window: LimitWindow;
  provider: AgentId;
  now: number;
}) {
  const { percent, note, text, color } = presentLimitWindow(window, now);
  return (
    <div className="flex flex-col gap-2 p-3.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{window.label}</span>
        {active && <ActiveAccountDot />}
      </div>
      <div className="flex items-baseline gap-2">
        <span
          className="inline-block shrink-0 px-1 py-0.5 text-[34px] leading-[1.2] font-semibold tabular-nums"
          style={{ color }}
        >
          {text}
        </span>
        {note ? (
          <span className="min-w-0 flex-1 pb-0.5 font-mono text-[12px] leading-snug text-[var(--mute)]">{note}</span>
        ) : null}
      </div>
      <Meter label={window.label} percent={percent} provider={provider} className="h-1.5" />
    </div>
  );
}

/** Remaining windows as compact meter rows, with the reset as the note. */
function WindowRow({
  window,
  provider,
  now,
}: {
  window: LimitWindow;
  provider: AgentId;
  now: number;
}) {
  const { percent, note, text, color } = presentLimitWindow(window, now);
  const awaitingFirstMessage = provider === "claude" && window.kind === "session"
    && window.usedPercent === 0 && window.resetsAt == null;
  const resetNote = note || (awaitingFirstMessage ? "Starts with your first message" : "Reset time unavailable");
  return <MeterRow label={window.label} note={resetNote} percent={percent} text={text} color={color} provider={provider} />;
}

