import { useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { confirmItem, type ActionMenuItem } from "@/components/ActionMenu";
import { actionButton as button } from "@/components/actionButton";
import { useAccountManagement, type AccountController } from "@/features/accounts/AccountManager";
import { SwitchAlongsideConfirmation } from "@/features/accounts/SwitchAlongsideConfirmation";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { providerLabel, type LimitsProvider } from "$lib/providers";
import { cachedAccountList } from "$lib/sharedReads/accounts";
import { checkLimitsNow, setCategoryLocally } from "$lib/sharedReads/limits";
import { AutomaticSpend, usePendingResetSpend } from "./AutomaticSpend";
import { UseBankedReset } from "./BankedResets";
import { cardActionPlan, hasResetControls, type CheckNowEntry, type MainEntry, type MenuEntry } from "./cardActionPlan";
import type { CardAccount, LimitCard } from "./limitCards";
import { ResetAlertForm } from "./resetAlerts";

export function removeAccountQuestion(label: string): string {
  return `Remove ${label} from on-n-off? You will need to sign in to add it again.`;
}

export type CardHandlers = {
  onArchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
};

export type CardFrameInput = { provider: LimitsProvider; card: LimitCard; now: number; checking: boolean } & CardHandlers;

export type CardActions = { menu: ActionMenuItem[]; footer: ReactNode };

function signingIn(manager: AccountController, accountId: string | null): "running" | "canceling" | null {
  if (accountId === null || manager.loginTarget !== accountId) return null;
  return manager.busy === "cancelLogin" ? "canceling" : manager.busy === "login" ? "running" : null;
}

export function useCheckNow(provider: LimitsProvider): (entry: CheckNowEntry) => ActionMenuItem {
  const client = useQueryClient();
  return ({ enabled }) => ({ id: "checkNow", label: "Check limits now", disabled: !enabled, run: () => void checkLimitsNow(client, provider).catch(() => {}) });
}

export function useCardActions({ provider, card, now, checking, onArchive, onRemove }: CardFrameInput): CardActions {
  const client = useQueryClient();
  const checkNow = useCheckNow(provider);
  const manager = useAccountManagement();
  const [archiving, setArchiving] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [clients, setClients] = useState<string[] | null>(null);
  const mainButton = useRef<HTMLButtonElement>(null);
  const accountId = card.account?.id ?? null;
  const spend = usePendingResetSpend(hasResetControls(card) ? accountId : null);
  const plan = cardActionPlan({
    card, now, checking, archiving, removing, pendingSpend: spend,
    accounts: manager && { controls: manager, blocked: manager.blocked, signingIn: signingIn(manager, accountId), runningClients: clients },
  });
  if (plan.account === null) return { menu: plan.menu.map(checkNow), footer: null };
  const { account, footer: { main, switchAlongside, resets } } = plan;

  function archive() {
    if (archiving) return;
    setArchiving(true);
    void onArchive(account).finally(() => setArchiving(false));
  }

  function startSwitch(controls: AccountController, profileId: string) {
    setClients(null);
    void controls.use(profileId).then(running => setClients(running.length ? running : null)).catch(() => {});
  }

  function menuItem(entry: MenuEntry<AccountController>): ActionMenuItem {
    switch (entry.action) {
      case "checkNow": return checkNow(entry);
      case "category": return {
        id: entry.action, label: "Edit category", disabled: !entry.enabled,
        panel: done => <CategoryPanel initial={entry.initial} disabled={!entry.enabled} done={done} onSave={async category => {
          const stored = await api.setAccountCategory(provider, account.id, category);
          setCategoryLocally(client, provider, account.id, stored);
        }} />,
      };
      case "signInAgain": return { id: entry.action, label: "Sign in again", disabled: !entry.enabled, run: () => void entry.controls.add(entry.profileId, account.id) };
      case "removeLogin": return confirmItem({
        id: entry.action, label: "Remove saved login", question: `Remove the saved login for ${account.name}? This account stays signed in.`, confirmLabel: "Confirm removal", disabled: !entry.enabled,
        onConfirm: async done => { await entry.controls.action("remove", entry.profileId); done(); },
      });
      case "archive": return { id: entry.action, label: "Archive account", disabled: !entry.enabled, busy: entry.busy, run: archive };
      case "resetAlert": return { id: entry.action, label: "Banked reset alert", panel: done => <ResetAlertForm accountId={account.id} label={account.name} onDone={done} /> };
      case "signOut": return confirmItem({
        id: entry.action, label: "Sign out", question: "Sign out of this account? The provider may also revoke its saved sign-ins.", confirmLabel: "Confirm sign out", disabled: !entry.enabled,
        onConfirm: async done => {
          if (entry.enabled && cachedAccountList(client, entry.controls.provider)?.nativeObservationId === account.id) await entry.controls.action("signOut");
          done();
        },
      });
      case "remove": return confirmItem({
        id: entry.action, label: "Remove account", question: removeAccountQuestion(account.name), confirmLabel: "Confirm removal", disabled: !entry.enabled,
        onConfirm: async done => {
          setRemoving(true);
          try { await onRemove(account); }
          finally { setRemoving(false); }
          done();
        },
      });
    }
  }

  function mainButtonFor(entry: MainEntry<AccountController>): ReactNode {
    const props = { ref: mainButton, className: button, disabled: !entry.enabled };
    switch (entry.action) {
      case "cancelSignIn": return <button {...props} onClick={() => void entry.controls.cancel()}>{entry.busy ? "Canceling…" : "Cancel sign-in"}</button>;
      case "signInAgain": return <button {...props} onClick={() => void entry.controls.add(entry.profileId, account.id)}>Sign in again</button>;
      case "archive": return <button {...props} aria-disabled={entry.busy || undefined} aria-busy={entry.busy || undefined} onClick={archive}>Archive account</button>;
      case "saveAccount": return <button {...props} onClick={() => void entry.controls.action("save").catch(() => {})}>Save account</button>;
      case "signIn": return <button {...props} onClick={() => void entry.controls.add(entry.profileId ?? undefined, account.id)}>Sign in</button>;
      case "useAccount": return <button {...props} onClick={() => startSwitch(entry.controls, entry.profileId)}>Use account</button>;
    }
  }

  return {
    menu: plan.menu.map(menuItem),
    footer: <>
      {main && mainButtonFor(main)}
      {switchAlongside && <SwitchAlongsideConfirmation product={providerLabel(provider)} clients={switchAlongside.clients} disabled={!switchAlongside.enabled}
        onSwitch={() => { setClients(null); void switchAlongside.controls.action("useAlongsideClients", switchAlongside.profileId).catch(() => {}); }}
        onCancel={() => { setClients(null); mainButton.current?.focus(); }} />}
      {resets && <>
        <UseBankedReset entry={resets.reading} label={account.name} now={now} offered={resets.useBankedReset !== null} disabled={!resets.useBankedReset?.enabled} />
        <AutomaticSpend accountId={account.id} dueAt={resets.automaticSpend?.dueAt ?? null} now={now} />
      </>}
    </>,
  };
}

function CategoryPanel({ initial, disabled, onSave, done }: { initial: string; disabled: boolean; onSave: (category: string) => Promise<void>; done: () => void }) {
  const [category, setCategory] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function save() {
    setError(null);
    setSaving(true);
    try {
      await onSave(category);
      done();
    } catch (reason) {
      setError(parseInvokeError(reason).message);
    } finally {
      setSaving(false);
    }
  }
  return <>
    <form role="group" aria-label="Edit account category" className="flex flex-wrap gap-2" onSubmit={event => { event.preventDefault(); void save(); }}>
      <input aria-label="Category (optional)" placeholder="Category (optional)" maxLength={100} value={category} onChange={event => setCategory(event.target.value)} className="w-full min-w-0 rounded border border-[var(--hair)] bg-transparent px-2 py-1.5 text-[12px]" />
      <button className={button} disabled={disabled || saving}>Save category</button><button type="button" className={button} onClick={done}>Cancel</button>
    </form>
    {error && <p role="alert" className="text-[12px] text-[var(--trip)]">{error}</p>}
  </>;
}
