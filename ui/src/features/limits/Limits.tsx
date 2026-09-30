import { AccountCardActions } from "@/features/accounts/AccountCardActions";
import { AccountControllers, AccountManager, useAccountManagement } from "@/features/accounts/AccountManager";
import { useMemo, useRef, useState, type ReactNode, type RefCallback, type RefObject } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { AddAccount } from "@/features/accounts/AddAccount";
import * as api from "$lib/api";
import type { AccountsReading } from "$lib/accountTypes";
import { displayError, parseInvokeError } from "$lib/error";
import { useFocusHandoff } from "$lib/focusHandoff";
import type { ProviderLimits } from "$lib/limitsTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId, LimitsPollMinutes, ResetAlert } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { ArchivedAccounts } from "./ArchivedAccounts";
import { limitColumn, type CardAccount, type CardWindow, type LimitCard } from "./limitCards";
import { AccountSubscriptionBadge } from "./SubscriptionBadge";
import { useLimitsProviders } from "./useLimitsProviders";
import { BankedResetsRow, ResetOfferRow } from "./BankedResets";
import { CodexAccountActions } from "./CodexAccountActions";
import { ResetAlertForm, ResetAlertsContext } from "./resetAlerts";
import { UsageStatusBadge } from "./UsageStatusBadge";
import { CreditsRows } from "./Credits";
import { Meter, MeterRow } from "./Meter";

const NO_ALERTS: Record<string, ResetAlert> = {};

export function Limits({ pollMinutes = 5, resetAlerts = NO_ALERTS, onResetAlertsChange }: {
  pollMinutes?: LimitsPollMinutes;
  /** Codex accounts whose banked reset is offered once they run low (`AppSettings.resetAlerts`). */
  resetAlerts?: Record<string, ResetAlert>;
  onResetAlertsChange?: (alerts: Record<string, ResetAlert>) => Promise<void>;
}) {
  const alerts = useMemo(() => ({
    alerts: resetAlerts,
    save: async (accountId: string, alert: ResetAlert | null) => {
      const next = { ...resetAlerts };
      if (alert) next[accountId] = alert;
      else delete next[accountId];
      await onResetAlertsChange?.(next);
    },
  }), [resetAlerts, onResetAlertsChange]);
  return (
    <ResetAlertsContext.Provider value={alerts}>
      <AccountControllers><LimitsContent pollMinutes={pollMinutes} /></AccountControllers>
    </ResetAlertsContext.Provider>
  );
}

function LimitsContent({ pollMinutes }: { pollMinutes: LimitsPollMinutes }) {
  // Only Claude and Codex carry a subscription the backend can read; the rest report `unsupported`.
  const { providers, loading, now } = useLimitsProviders(pollMinutes);
  const addAccount = useRef<HTMLButtonElement | null>(null);

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
        <AddAccount buttonRef={node => { addAccount.current = node; }} />
      </header>

      <div className="grid items-start gap-3 lg:grid-cols-2">
        {providers.map(({ provider, query }) => (
          <div key={provider} className="flex flex-col gap-3">
            {(provider === "claude" || provider === "codex") ? <AccountManager provider={provider}>
              <ProviderColumn provider={provider} query={query} now={now} addAccount={addAccount} />
            </AccountManager> : <ProviderColumn provider={provider} query={query} now={now} addAccount={addAccount} />}
          </div>
        ))}
      </div>

      <p className="font-mono text-[11px] leading-snug text-[var(--mute)]">
        Reset times apply to usage limits. Signed-out accounts show their last known usage.
      </p>
    </div>
  );
}

/**
 * The current account card for a provider plus one card per remembered account, then the accounts
 * the user archived, collapsed.
 */
function ProviderColumn({
  provider,
  query,
  now,
  addAccount,
}: {
  provider: AgentId;
  query: UseQueryResult<ProviderLimits[]>;
  now: number;
  /** The screen's Add account button, where focus goes once no card is left to take it. */
  addAccount: RefObject<HTMLButtonElement | null>;
}) {
  const queryClient = useQueryClient();
  const name = providerLabel(provider);
  const manager = useAccountManagement();
  const column = limitColumn({ provider, entries: query.data, profiles: manager?.query.data?.profiles ?? [], now });
  const [forgetError, setForgetError] = useState<string | null>(null);
  const archivedDisclosure = useRef<HTMLButtonElement | null>(null);
  const menus = useRef(new Map<string, HTMLButtonElement>());
  const visibleKeys = column?.visible.map(card => card.key) ?? [];
  const menu = (key: string | undefined) => (key === undefined ? undefined : menus.current.get(key));
  // A card leaving hands focus to the archived list if it was archived, else to the next card's More
  // actions, else the previous one's.
  const handOffCard = useFocusHandoff<"archive" | "remove">(visibleKeys, (index, action) =>
    [action === "archive" ? archivedDisclosure.current : null, menu(visibleKeys[index]), menu(visibleKeys[index - 1]), addAccount.current]);
  const error = query.error ? displayError(parseInvokeError(query.error), name) : forgetError;
  const blocked = manager?.blocked ?? false;

  async function forget({ forget: steps }: CardAccount) {
    setForgetError(null);
    // Keep the verified association from the confirmed card even after its saved login is removed.
    const ids = steps.map(({ accountId }) => accountId);
    try {
      for (const { accountId, expectedEmail } of steps) await api.forgetLimitsSnapshot(provider, accountId, expectedEmail);
      queryClient.setQueryData<ProviderLimits[]>(["limits", provider], (current) =>
        current?.filter((entry) => entry.currentAccount || !entry.account || !ids.includes(entry.account.id)),
      );
    } catch (reason: unknown) {
      setForgetError(`Could not remove that account: ${displayError(parseInvokeError(reason), name)}`);
      throw reason;
    }
  }

  /** Archive from a card's menu or footer; once the card goes, focus follows it to the archived list. */
  async function archive(key: string, account: CardAccount) {
    try {
      await setArchived(account, true);
    } catch {
      return; // setArchived says why
    }
    handOffCard(key, "archive");
  }

  /** Remove account from a card's menu, whose last step drops the card; focus then goes on to its neighbour. */
  async function forgetCard(key: string, account: CardAccount) {
    await forget(account);
    handOffCard(key, "remove");
  }

  async function remove(account: CardAccount) {
    if (manager) await manager.removeAccount(account.profile?.id, () => forget(account));
    else await forget(account);
  }

  /**
   * Archive or unarchive every id the card stands for, its merged legacy history included; the card
   * moves at once. Unarchiving also reads the provider again in the backend, which announces it.
   */
  async function setArchived({ forget: steps }: CardAccount, archived: boolean) {
    setForgetError(null);
    const ids = steps.map(({ accountId }) => accountId);
    try {
      await api.setLimitsArchived(provider, ids, archived);
    } catch (reason: unknown) {
      const verb = archived ? "archive" : "unarchive";
      setForgetError(`Could not ${verb} that account: ${displayError(parseInvokeError(reason), name)}`);
      throw reason;
    }
    queryClient.setQueryData<ProviderLimits[]>(["limits", provider], (current) => current?.map((entry) =>
      !entry.currentAccount && entry.account && ids.includes(entry.account.id) ? { ...entry, archived } : entry));
    queryClient.setQueryData<AccountsReading>(["accounts", provider], (current) => current && {
      ...current,
      profiles: current.profiles.map((profile) => ids.includes(profile.observationId) ? { ...profile, archived } : profile),
    });
  }

  if (!column) {
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
      {column.visible.map((card, index) => (
        <AccountCard
          key={card.key}
          card={card}
          now={now}
          error={index === 0 ? error : null}
          onForget={(account) => forgetCard(card.key, account)}
          onArchive={(account) => archive(card.key, account)}
          menuButtonRef={(node) => {
            if (node) menus.current.set(card.key, node);
            else menus.current.delete(card.key);
          }}
        />
      ))}
      <ArchivedAccounts provider={provider} cards={column.archived} blocked={blocked}
        onUnarchive={(account) => setArchived(account, false)} onRemove={remove} focusWhenEmpty={() => menu(visibleKeys[0])}
        disclosureRef={(node) => { archivedDisclosure.current = node; }} />
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
  onArchive,
  menuButtonRef,
}: {
  card: LimitCard;
  now: number;
  error: string | null;
  onForget: (account: CardAccount) => Promise<void>;
  onArchive: (account: CardAccount) => Promise<void>;
  menuButtonRef: RefCallback<HTMLButtonElement>;
}) {
  const { provider, identity, freshness, figures, account } = card;
  const codexActions = account?.codexActions ?? null;
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
          className={`px-3.5 ${card.headline || card.rows.length > 0 ? "pt-1.5" : "py-4"} text-[13px] ${freshness.message.tone === "error" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}
        >
          {freshness.message.text}
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
      <BankedResetsRow resetCredits={figures.bankedResets} now={now} />
      <ResetOfferRow offer={figures.paidOffer} />
  </>;
  return (
    <section
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={identity.ariaLabel}
      data-status={freshness.readStatus}
    >
      {account ? <AccountCardActions accountId={account.id} label={account.name} current={card.active} profile={account.profile ?? undefined}
        onForget={() => onForget(account)}
        onArchive={() => onArchive(account)} archiveInsteadOfUse={card.archiveInsteadOfUse} menuButtonRef={menuButtonRef} header={header}
        extraAction={provider === "codex" ? {
          label: "Banked reset alert",
          render: close => <ResetAlertForm accountId={account.id} label={account.name} onDone={close} />,
        } : undefined}
        footer={codexActions ? state => <CodexAccountActions entry={codexActions} label={account.name} now={now} state={state} /> : undefined}>
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
