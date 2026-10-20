import { AccountControllers, AccountManager, useAccountManagement } from "@/features/accounts/AccountManager";
import { useMemo, useRef, useState, type RefCallback, type RefObject } from "react";
import { useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { AddAccount } from "@/features/accounts/AddAccount";
import * as api from "$lib/api";
import { withResetAlert } from "$lib/appSettings";
import { displayError, parseInvokeError } from "$lib/error";
import { useFocusHandoff } from "$lib/focusHandoff";
import type { ProviderLimits } from "$lib/limitsTypes";
import { accountCategoriesQuery, markProfilesArchived } from "$lib/sharedReads/accounts";
import { updateLimits } from "$lib/sharedReads/limits";
import type { AgentId, LimitsPollMinutes, ResetAlert } from "$lib/types";
import { hasSavedAccounts, providerLabel, type LimitsProvider } from "$lib/providers";
import { ArchivedAccounts } from "./ArchivedAccounts";
import { ActiveAccountDot, LimitCardFrame, PendingCardFrame } from "./LimitCardFrame";
import { isSignedIn, limitColumn, type CardAccount, type CardWindow, type LimitCard } from "./limitCards";
import { useLimitsProviders } from "./useLimitsProviders";
import { BankedResetsRow, ResetOfferRow } from "./BankedResets";
import { ResetAlertsContext } from "./resetAlerts";
import { CreditsRows } from "./Credits";
import { Meter, MeterRow } from "./Meter";

export function Limits({ pollMinutes = 5, resetAlerts, onResetAlertsChange }: {
  pollMinutes?: LimitsPollMinutes;
  resetAlerts: Record<string, ResetAlert>;
  onResetAlertsChange: (alerts: Record<string, ResetAlert>) => Promise<void>;
}) {
  const alerts = useMemo(() => ({
    alerts: resetAlerts,
    save: (accountId: string, alert: ResetAlert | null) => onResetAlertsChange(withResetAlert(resetAlerts, accountId, alert)),
  }), [resetAlerts, onResetAlertsChange]);
  return (
    <ResetAlertsContext.Provider value={alerts}>
      <AccountControllers><LimitsContent pollMinutes={pollMinutes} /></AccountControllers>
    </ResetAlertsContext.Provider>
  );
}

function LimitsContent({ pollMinutes }: { pollMinutes: LimitsPollMinutes }) {
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

      <div className="grid grid-flow-col auto-cols-fr items-start gap-3">
        {providers.map(({ provider, query }) => (
          <div key={provider} className="flex flex-col gap-3">
            {hasSavedAccounts(provider) ? <AccountManager provider={provider}>
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

function ProviderColumn({
  provider,
  query,
  now,
  addAccount,
}: {
  provider: LimitsProvider;
  query: UseQueryResult<ProviderLimits[]>;
  now: number;
  addAccount: RefObject<HTMLButtonElement | null>;
}) {
  const queryClient = useQueryClient();
  const name = providerLabel(provider);
  const manager = useAccountManagement();
  const categories = useQuery(accountCategoriesQuery()).data?.[provider];
  const column = limitColumn({ provider, entries: query.data, profiles: manager?.query.data?.profiles ?? [], categories, nativeLogin: manager?.nativeLogin ?? null, now });
  const [forgetError, setForgetError] = useState<string | null>(null);
  const archivedDisclosure = useRef<HTMLButtonElement | null>(null);
  const menus = useRef(new Map<string, HTMLButtonElement>());
  const visibleKeys = column?.visible.map(card => card.key) ?? [];
  const menu = (key: string | undefined) => (key === undefined ? undefined : menus.current.get(key));
  const handOffCard = useFocusHandoff<"archive" | "remove">(visibleKeys, (index, action) =>
    [action === "archive" ? archivedDisclosure.current : null, menu(visibleKeys[index]), menu(visibleKeys[index - 1]), addAccount.current]);
  const error = query.error ? displayError(parseInvokeError(query.error), name) : forgetError;
  const blocked = manager?.blocked ?? false;

  async function forget({ forget: steps }: CardAccount) {
    setForgetError(null);
    const ids = steps.map(({ accountId }) => accountId);
    try {
      for (const { accountId, expectedEmail } of steps) await api.forgetLimitsSnapshot(provider, accountId, expectedEmail);
      updateLimits(queryClient, provider, (entries) =>
        entries.filter((entry) => entry.currentAccount || !entry.account || !ids.includes(entry.account.id)),
      );
    } catch (reason: unknown) {
      setForgetError(`Could not remove that account: ${displayError(parseInvokeError(reason), name)}`);
      throw reason;
    }
  }

  async function archive(key: string, account: CardAccount) {
    try {
      await setArchived(account, true);
    } catch {
      return;
    }
    handOffCard(key, "archive");
  }

  async function forgetCard(key: string, account: CardAccount) {
    await forget(account);
    handOffCard(key, "remove");
  }

  async function remove(account: CardAccount, forgetIt = () => forget(account)) {
    if (manager) await manager.removeAccount(account.profile?.id, forgetIt);
    else await forgetIt();
  }

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
    updateLimits(queryClient, provider, (entries) => entries.map((entry) =>
      !entry.currentAccount && entry.account && ids.includes(entry.account.id) ? { ...entry, archived } : entry));
    if (manager) markProfilesArchived(queryClient, manager.provider, ids, archived);
  }

  if (!column) {
    return (
      <PendingCardFrame provider={provider} now={now} checking={query.isFetching}>
        {error ? <p className="px-3.5 pt-3 text-[13px] text-[var(--trip)]">{error}</p> : null}
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{query.isFetching ? "Checking limits…" : "No data yet."}</p>
      </PendingCardFrame>
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
          checking={query.isFetching}
          onArchive={(account) => archive(card.key, account)}
          onRemove={(account) => remove(account, () => forgetCard(card.key, account))}
          menuButtonRef={(node) => {
            if (node) menus.current.set(card.key, node);
            else menus.current.delete(card.key);
          }}
        />
      ))}
      <ArchivedAccounts provider={provider} cards={column.archived} blocked={blocked}
        onUnarchive={(account) => setArchived(account, false)} onRemove={(account) => remove(account)} focusWhenEmpty={() => menu(visibleKeys[0])}
        disclosureRef={(node) => { archivedDisclosure.current = node; }} />
    </div>
  );
}

function AccountCard({
  card,
  now,
  error,
  checking,
  onArchive,
  onRemove,
  menuButtonRef,
}: {
  card: LimitCard;
  now: number;
  error: string | null;
  checking: boolean;
  onArchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
  menuButtonRef: RefCallback<HTMLButtonElement>;
}) {
  const { provider, freshness, figures } = card;
  return (
    <LimitCardFrame provider={provider} card={card} now={now} checking={checking} menuButtonRef={menuButtonRef} onArchive={onArchive} onRemove={onRemove}>
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

      {card.headline ? <HeadlineWindow signedIn={isSignedIn(card)} window={card.headline} provider={provider} /> : null}
      {card.rows.map((row) => (
        <WindowRow key={row.id} row={row} provider={provider} />
      ))}
      {card.empty ? (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{card.empty.copy}</p>
      ) : null}

      <CreditsRows figures={figures} provider={provider} now={now} />
      <BankedResetsRow resetCredits={figures.bankedResets} now={now} />
      <ResetOfferRow offer={figures.paidOffer} />
    </LimitCardFrame>
  );
}

function HeadlineWindow({
  signedIn,
  window,
  provider,
}: {
  signedIn: boolean;
  window: CardWindow;
  provider: AgentId;
}) {
  const { label, percent, meter, note, text, color } = window;
  return (
    <div className="flex flex-col gap-2 p-3.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{label}</span>
        {signedIn && <ActiveAccountDot />}
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
      <Meter label={label} percent={percent} meter={meter} provider={provider} className="h-1.5" />
    </div>
  );
}

function WindowRow({ row, provider }: { row: CardWindow; provider: AgentId }) {
  return <MeterRow label={row.label} note={row.note} percent={row.percent} meter={row.meter} text={row.text} color={row.color} provider={provider} />;
}
