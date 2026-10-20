import { useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { confirmItem, type ActionMenuItem } from "@/components/ActionMenu";
import { actionButton as button } from "@/components/actionButton";
import { useAccountManagement } from "@/features/accounts/AccountManager";
import { SwitchAlongsideConfirmation } from "@/features/accounts/SwitchAlongsideConfirmation";
import * as api from "$lib/api";
import { parseInvokeError } from "$lib/error";
import { providerLabel, type LimitsProvider } from "$lib/providers";
import { cachedAccountList, setCategoryLocally } from "$lib/sharedReads/accounts";
import { checkLimitsNow } from "$lib/sharedReads/limits";
import { AutomaticSpend, usePendingResetSpend } from "./AutomaticSpend";
import { UseBankedReset } from "./BankedResets";
import { cardActionPlan, hasResetControls, type MainAction, type MenuAction, type PlanEntry } from "./cardActionPlan";
import type { CardAccount, LimitCard } from "./limitCards";
import { ResetAlertForm } from "./resetAlerts";

export function removeAccountQuestion(label: string): string {
  return `Remove ${label} from on-n-off? You will need to sign in to add it again.`;
}

export type CardHandlers = {
  onArchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
};

export type CardActionsInput = { provider: LimitsProvider; now: number; checking: boolean }
  & ({ card: null } | ({ card: LimitCard } & CardHandlers));

export type CardActions = { menu: ActionMenuItem[]; footer: ReactNode };

export function useCardActions(input: CardActionsInput): CardActions {
  const { provider, now, checking, card } = input;
  const client = useQueryClient();
  const manager = useAccountManagement();
  const [archiving, setArchiving] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [clients, setClients] = useState<string[] | null>(null);
  const mainButton = useRef<HTMLButtonElement>(null);
  const account = card?.account ?? null;
  const spend = usePendingResetSpend(account && hasResetControls(card) ? account.id : null);
  const profile = account?.profile ?? null;
  const signingIn = manager && account && manager.loginTarget === account.id
    ? manager.busy === "cancelLogin" ? "canceling" as const : manager.busy === "login" ? "running" as const : null
    : null;
  const plan = cardActionPlan({
    card, now, checking, archiving, removing, pendingSpend: spend !== null,
    accounts: manager && { blocked: manager.blocked, signingIn, runningClients: clients !== null },
  });
  const checkNow = (enabled: boolean): ActionMenuItem => ({ id: "checkNow", label: "Check limits now", disabled: !enabled, run: () => void checkLimitsNow(client, provider).catch(() => {}) });
  if (!input.card || !account) return { menu: plan.menu.map(entry => checkNow(entry.enabled)), footer: null };
  const { onArchive, onRemove } = input;
  const shown = input.card;
  const own: CardAccount = account;
  const name = own.name;

  function archive() {
    if (archiving) return;
    setArchiving(true);
    void onArchive(own).finally(() => setArchiving(false));
  }

  function startSwitch(profileId: string) {
    if (!manager) return;
    setClients(null);
    void manager.use(profileId).then(running => setClients(running.length ? running : null)).catch(() => {});
  }

  function menuItem({ action, enabled, busy }: PlanEntry<MenuAction>): ActionMenuItem {
    const disabled = !enabled;
    switch (action) {
      case "checkNow": return checkNow(enabled);
      case "category": return {
        id: action, label: "Edit category", disabled,
        panel: done => <CategoryPanel initial={shown.identity.category ?? ""} disabled={disabled} done={done} onSave={async category => {
          const stored = await api.setAccountCategory(provider, own.id, category);
          setCategoryLocally(client, provider, own.id, stored);
        }} />,
      };
      case "signInAgain": return { id: action, label: "Sign in again", disabled, run: () => void manager?.add(profile?.id, own.id) };
      case "removeLogin": return confirmItem({
        id: action, label: "Remove saved login", question: `Remove the saved login for ${name}? This account stays signed in.`, confirmLabel: "Confirm removal", disabled,
        onConfirm: async done => { if (manager && profile) await manager.action("remove", profile.id); done(); },
      });
      case "archive": return { id: action, label: "Archive account", disabled, busy, run: archive };
      case "resetAlert": return { id: action, label: "Banked reset alert", disabled, panel: done => <ResetAlertForm accountId={own.id} label={name} onDone={done} /> };
      case "signOut": return confirmItem({
        id: action, label: "Sign out", question: "Sign out of this account? The provider may also revoke its saved sign-ins.", confirmLabel: "Confirm sign out", disabled,
        onConfirm: async done => {
          if (manager && shown.signedIn === "confirmed" && cachedAccountList(client, manager.provider)?.nativeObservationId === own.id) await manager.action("signOut");
          done();
        },
      });
      case "remove": return confirmItem({
        id: action, label: "Remove account", question: removeAccountQuestion(name), confirmLabel: "Confirm removal", disabled,
        onConfirm: async done => {
          setRemoving(true);
          try { await onRemove(own); }
          finally { setRemoving(false); }
          done();
        },
      });
    }
  }

  function mainButtonFor({ action, enabled, busy }: PlanEntry<MainAction>): ReactNode {
    const props = { ref: mainButton, className: button, disabled: !enabled };
    switch (action) {
      case "cancelSignIn": return <button {...props} onClick={() => void manager?.cancel()}>{busy ? "Canceling…" : "Cancel sign-in"}</button>;
      case "signInAgain": return <button {...props} onClick={() => void manager?.add(profile?.id, own.id)}>Sign in again</button>;
      case "archive": return <button {...props} aria-disabled={busy || undefined} aria-busy={busy || undefined} onClick={archive}>Archive account</button>;
      case "saveAccount": return <button {...props} onClick={() => void manager?.action("save").catch(() => {})}>Save account</button>;
      case "signIn": return <button {...props} onClick={() => void manager?.add(profile?.id, own.id)}>Sign in</button>;
      case "useAccount": return <button {...props} onClick={() => profile && startSwitch(profile.id)}>Use account</button>;
    }
  }

  const { main, switchAlongside, resets } = plan.footer;
  return {
    menu: plan.menu.map(menuItem),
    footer: <>
      {main && mainButtonFor(main)}
      {switchAlongside && clients && profile && <SwitchAlongsideConfirmation product={providerLabel(provider)} clients={clients} disabled={!switchAlongside.enabled}
        onSwitch={() => { setClients(null); void manager?.action("useAlongsideClients", profile.id).catch(() => {}); }}
        onCancel={() => { setClients(null); mainButton.current?.focus(); }} />}
      {resets && shown.reading && <>
        <UseBankedReset entry={shown.reading} label={name} now={now} offered={resets.useBankedReset !== null} disabled={!resets.useBankedReset?.enabled} />
        <AutomaticSpend accountId={own.id} dueAt={resets.cancelAutomaticSpend && spend ? spend.dueAt : null} now={now} />
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
