import { AccountCardActions } from "@/features/accounts/AccountCardActions";
import type { SavedProfile } from "$lib/accountTypes";
import { AccountControllers, AccountManager, useAccountManagement } from "@/features/accounts/AccountManager";
import { useState, type ReactNode } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { AddAccount } from "@/features/accounts/AddAccount";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import {
  planLabel,
  usageFillColor,
  type UsageTone,
} from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { presentLimitAccount, presentLimitWindow, visibleLimitWindows } from "./limitPresentation";
import { CodexSubscriptionBadge } from "./SubscriptionBadge";
import { useLimitsProviders } from "./useLimitsProviders";
import { accountCards } from "./accountCards";

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
  const entries = query.data || profiles.length ? cards.entries : null;
  if (entries) for (const profile of profiles) {
    if (profile.observationId && !entries.some(entry => entry.account?.id === profile.observationId)) {
      entries.push({ provider, status: "ok", account: { id: profile.observationId, label: profile.email }, currentAccount: profile.active, windows: [], message: "Usage unavailable." });
    }
  }
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

  const rows = entries.map(entry => {
    const profile = profiles.find(profile => profile.observationId === entry.account?.id);
    return { entry, profile, label: profile?.email ?? entry.account?.label };
  });
  const workspacesByEmail = new Map<string, Set<string>>();
  for (const profile of profiles) {
    if (!profile.email) continue;
    const email = profile.email.trim().toLowerCase();
    const workspaces = workspacesByEmail.get(email) ?? new Set<string>();
    workspaces.add(profile.identity.workspaceId);
    workspacesByEmail.set(email, workspaces);
  }
  return (
    <div className="flex flex-col gap-3">
      {rows.map(({ entry, profile, label }, index) => (
        <AccountCard
          key={`${entry.currentAccount ? "current" : "remembered"}-${entry.account?.id ?? index}`}
          entry={entry}
          profile={profile}
          distinguishWorkspace={!!label && (workspacesByEmail.get(label.trim().toLowerCase())?.size ?? 0) > 1}
          now={now}
          error={index === 0 ? error : null}
          onForget={allowForget ? forget : undefined}
        />
      ))}
    </div>
  );
}

/** Account identity stays prominent; workspace appears only when the email is ambiguous. */
function CardHeader({ entry, provider, updatedAt, subscription, profile, distinguishWorkspace, menu, activeWithoutUsage }: { entry?: ProviderLimits; provider: AgentId; updatedAt?: string | null; subscription?: ReactNode; profile?: SavedProfile; distinguishWorkspace?: boolean; menu?: ReactNode; activeWithoutUsage?: boolean }) {
  const name = providerLabel(provider);
  const label = profile?.email ?? entry?.account?.label ?? null;
  const plan = planLabel(entry?.plan, provider);
  const credits = entry?.credits ?? null;
  return (
    <header className="border-b border-[var(--hair)] px-3.5 py-2.5" title={updatedAt ? `Usage last checked ${updatedAt}` : undefined}>
      <div className="flex items-center gap-2.5">
        <ProviderIcon provider={provider} className="size-3.5 shrink-0 translate-y-[0.5px]" />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-semibold" title={label ?? name}>{label ?? name}</div>
        </div>
        {credits ? (
          <span className="font-mono text-[11px] text-[var(--mute)]">
            {credits.unlimited ? "unlimited credits" : `${credits.balance} credits`}
          </span>
        ) : null}
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
        {activeWithoutUsage && <ActiveAccountDot />}
        {subscription}
        {plan ? (
          <span className="rounded-md border border-[var(--hair)] px-1.5 py-0.5 type-badge uppercase">
            {plan}
          </span>
        ) : null}
        </div>
        {menu}
      </div>
      <div className="pl-6">
        {profile?.category && <div className="mt-0.5 break-words text-[11px] text-[var(--mute)]">{profile.category}</div>}
        {distinguishWorkspace && profile && <div className="mt-0.5 break-all text-[11px] text-[var(--mute)]">Workspace · {profile.identity.workspaceId}</div>}
      </div>
      {updatedAt ? <p className="sr-only">Latest observation {updatedAt}</p> : null}
    </header>
  );
}

function AccountCard({
  entry,
  profile,
  distinguishWorkspace,
  now,
  error,
  onForget,
}: {
  entry: ProviderLimits;
  profile?: SavedProfile;
  distinguishWorkspace?: boolean;
  now: number;
  error: string | null;
  onForget?: (accountId: string) => Promise<void>;
}) {
  const name = providerLabel(entry.provider);
  const account = entry.account ?? null;
  const label = profile?.email ?? account?.label ?? null;
  const title = label ? `${name} limits · ${label}` : `${name} limits`;
  const [hero, ...rest] = visibleLimitWindows(entry);
  const active = !!account && (profile?.active ?? entry.currentAccount);
  const { message, refreshPaused, updatedAt } = presentLimitAccount(entry, `${name} limits are unavailable.`);

  const subscription = entry.provider === "codex" && account
    ? <CodexSubscriptionBadge accountId={account.id} current={entry.currentAccount} now={now} />
    : null;
  const header = (menu: ReactNode) => <CardHeader activeWithoutUsage={active && !hero} menu={menu} entry={entry} provider={entry.provider} updatedAt={updatedAt} subscription={subscription} profile={profile} distinguishWorkspace={distinguishWorkspace} />;
  const content = <>
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
          <HeroWindow active={active} window={hero} provider={entry.provider} now={now} />
          {rest.map((window) => (
            <WindowRow
              key={window.id}
              window={window}
              provider={entry.provider}
              now={now}
            />
          ))}
        </>
      ) : entry.status === "ok" && !message ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{profile ? "Usage unavailable." : `${name} reported no rate-limit windows.`}</p>
      ) : null}
  </>;
  return (
    <section
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={title}
      data-status={entry.status}
      data-current-account={entry.currentAccount ? "true" : "false"}
    >
      {account ? <AccountCardActions accountId={account.id} label={label ?? account.id} current={profile?.active ?? entry.currentAccount} profile={profile} onForget={onForget} header={header}>
        {content}
      </AccountCardActions> : <>{header(null)}{content}</>}
    </section>
  );
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
      <div
        className="h-full rounded-sm transition-[width]"
        style={{ width: `${percent}%`, background: usageFillColor(provider, tone) }}
      />
    </div>
  );
}

function ActiveAccountDot() {
  return <span role="img" aria-label="Active account" title="Active account"
    className="size-[7px] shrink-0 rounded-full bg-[var(--live)] shadow-[0_0_8px_var(--live)]" />;
}

/** The first (weekly) window as the card's headline, in the Overview's big-number idiom. */
function HeroWindow({
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
  const { percent, tone, note, text, color } = presentLimitWindow(window, now);
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
      <Meter window={window} percent={percent} tone={tone} provider={provider} className="h-1.5" />
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
  const { percent, tone, note, text, color } = presentLimitWindow(window, now);
  const awaitingFirstMessage = provider === "claude" && window.kind === "session"
    && window.usedPercent === 0 && window.resetsAt == null;
  const resetNote = note || (awaitingFirstMessage ? "Starts with your first message" : "Reset time unavailable");
  return (
    <div className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span
          className="text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase"
        >
          {window.label}
        </span>
        <span className="font-mono text-[11px] leading-snug text-[var(--mute)]">{resetNote}</span>
      </div>
      <Meter window={window} percent={percent} tone={tone} provider={provider} className="h-1 w-24 shrink-0" />
      <span
        className="w-11 shrink-0 text-right font-mono text-[12px]"
        style={{ color }}
      >
        {text}
      </span>
    </div>
  );
}
