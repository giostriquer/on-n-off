import { AccountCardActions } from "@/features/accounts/AccountCardActions";
import { AccountControllers, AccountManager, useAccountManagement } from "@/features/accounts/AccountManager";
import { useState, type ReactNode } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { AddAccount } from "@/features/accounts/AddAccount";
import * as api from "$lib/api";
import { displayError, parseInvokeError } from "$lib/error";
import type { ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { limitCards, type CardWindow, type LimitCard } from "./limitCards";
import { AccountSubscriptionBadge } from "./SubscriptionBadge";
import { useLimitsProviders } from "./useLimitsProviders";
import { BankedResetsRow, ResetOfferRow } from "./BankedResets";
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
  const cards = limitCards({ provider, entries: query.data, profiles: manager?.query.data?.profiles ?? [], now });
  const [forgetError, setForgetError] = useState<string | null>(null);
  const error = query.error ? displayError(parseInvokeError(query.error), name) : forgetError;

  async function forget({ forget: steps }: LimitCard) {
    setForgetError(null);
    // Keep the verified association from the confirmed card even after its saved login is removed.
    const ids = steps.map(([accountId]) => accountId);
    try {
      for (const step of steps) await api.forgetLimitsSnapshot(provider, ...step);
      queryClient.setQueryData<ProviderLimits[]>(["limits", provider], (current) =>
        current?.filter((entry) => entry.currentAccount || !entry.account || !ids.includes(entry.account.id)),
      );
    } catch (reason: unknown) {
      setForgetError(`Could not remove that account: ${displayError(parseInvokeError(reason), name)}`);
      throw reason;
    }
  }

  if (!cards) {
    return (
      <section
        className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
        aria-label={`${name} limits`}
        data-status="pending"
      >
        <CardHeader provider={provider} title={name} />
        {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{query.isFetching ? "Checking limits…" : "No data yet."}</p>
      </section>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {cards.map((card, index) => (
        <AccountCard
          key={card.key}
          card={card}
          now={now}
          error={index === 0 ? error : null}
          onForget={allowForget ? () => forget(card) : undefined}
        />
      ))}
    </div>
  );
}

/** Account identity stays prominent; workspace ids are never displayed. */
function CardHeader({ card, provider, title, subscription, menu }: { card?: LimitCard; provider: AgentId; title: string; subscription?: ReactNode; menu?: ReactNode }) {
  const updatedAt = card?.freshness.updatedAt;
  const status = card?.status;
  return (
    <header className="border-b border-[var(--hair)] px-3.5 py-2.5" title={updatedAt ? `Usage last checked ${updatedAt}` : undefined}>
      <div className="flex items-center gap-2.5">
        <ProviderIcon provider={provider} className="size-3.5 shrink-0 translate-y-[0.5px]" />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-semibold" title={title}>{title}</div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
        {card?.active && !card.headline && <ActiveAccountDot />}
        {subscription}
        {status?.kind === "savedRefresh" && <UsageStatusBadge detail={status.detail} />}
        {card?.plan ? (
          <span className="rounded-md border border-[var(--hair)] px-1.5 py-0.5 type-badge uppercase">
            {card.plan}
          </span>
        ) : null}
        </div>
        {menu}
      </div>
      {card?.identity.category && <div className="mt-0.5 break-words pl-6 text-[11px] text-[var(--mute)]">{card.identity.category}</div>}
      {updatedAt ? <p className="sr-only">Latest observation {updatedAt}</p> : null}
    </header>
  );
}

function AccountCard({
  card,
  now,
  error,
  onForget,
}: {
  card: LimitCard;
  now: number;
  error: string | null;
  onForget?: () => Promise<void>;
}) {
  const { provider, identity, freshness, figures, reading } = card;
  const { accountName } = identity;
  const subscription = <AccountSubscriptionBadge subscription={card.subscription} now={now} />;
  const header = (menu: ReactNode) => <CardHeader card={card} provider={provider} title={identity.label} subscription={subscription} menu={menu} />;
  const content = <>
      {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}

      {card.status?.kind === "paused" ? (
        <p className="px-3.5 pt-3 text-[13px] font-medium text-[var(--silkscreen)]">Refresh paused.</p>
      ) : card.status?.kind === "remembered" ? (
        <p className="px-3.5 pt-3 text-[13px] text-[var(--mute)]">Remembered account.</p>
      ) : null}

      {freshness.message ? (
        <p
          className={`px-3.5 ${card.headline || card.rows.length > 0 ? "pt-1.5" : "py-4"} text-[13px] ${freshness.failed ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}
        >
          {freshness.message}
        </p>
      ) : null}

      {card.headline ? <HeadlineWindow active={card.active} window={card.headline} provider={provider} /> : null}
      {card.rows.map((row) => (
        <WindowRow key={row.id} row={row} provider={provider} />
      ))}
      {card.empty ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{card.empty.copy}</p>
      ) : null}

      <CreditsRows figures={figures} provider={provider} now={now} />
      <BankedResetsRow resetCredits={figures.bankedResets?.resetCredits} hint={figures.bankedResets?.hint} now={now} />
      <ResetOfferRow offer={figures.paidOffer} />
  </>;
  return (
    <section
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={identity.ariaLabel}
      data-status={freshness.readStatus}
    >
      {card.accountId !== null && accountName !== null ? <AccountCardActions accountId={card.accountId} label={accountName} current={card.active} profile={card.profile ?? undefined} onForget={onForget} header={header}
        footer={card.resetAction && reading ? state => <CodexAccountActions entry={reading} label={accountName} now={now} state={state} /> : undefined}>
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
}: {
  active: boolean;
  window: CardWindow;
  provider: AgentId;
}) {
  const { label, percent, note, text, color } = window;
  return (
    <div className="flex flex-col gap-2 p-3.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{label}</span>
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
      <Meter label={label} percent={percent} provider={provider} className="h-1.5" />
    </div>
  );
}

/** Remaining windows as compact meter rows, with the reset, or why there is none, as the note. */
function WindowRow({ row, provider }: { row: CardWindow; provider: AgentId }) {
  return <MeterRow label={row.label} note={row.note} percent={row.percent} text={row.text} color={row.color} provider={provider} />;
}
