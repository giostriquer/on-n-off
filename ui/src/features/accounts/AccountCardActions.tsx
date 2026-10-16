import { useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { SavedProfile } from "$lib/accountTypes";
import { parseInvokeError } from "$lib/error";
import { providerLabel } from "$lib/providers";
import { cachedAccountList } from "$lib/sharedReads/accounts";
import type { CardMenuItem } from "@/features/limits/CardMenu";
import { accountButton as button, useAccountManagement } from "./AccountManager";

export type AccountFooterState = { current: boolean; blocked: boolean; unconfirmedCurrent: boolean };

export type AccountCardInput = {
  accountId: string;
  label: string;
  current: boolean;
  profile?: SavedProfile;
  onForget: () => Promise<void>;
  onArchive: () => Promise<void>;
  archiveInsteadOfUse?: boolean;
  signInAgain?: boolean;
  footer?: (state: AccountFooterState) => ReactNode;
};

export type AccountCardActions = { items: CardMenuItem[]; danger: CardMenuItem[]; footer: ReactNode };

export function removeAccountQuestion(label: string): string {
  return `Remove ${label} from on-n-off? You will need to sign in to add it again.`;
}

const NO_ACTIONS: AccountCardActions = { items: [], danger: [], footer: null };

export function useAccountCardActions(account: AccountCardInput | null): AccountCardActions {
  const manager = useAccountManagement();
  const client = useQueryClient();
  const [removing, setRemoving] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const [clients, setClients] = useState<string[] | null>(null);
  const useButton = useRef<HTMLButtonElement>(null);
  if (!account) return NO_ACTIONS;
  const { accountId, label, current, profile, onForget, onArchive, archiveInsteadOfUse = false, signInAgain = false, footer } = account;
  const disabled = (manager?.blocked ?? false) || removing;
  async function whileRemoving(run: () => Promise<void>) {
    setRemoving(true);
    try { await run(); }
    finally { setRemoving(false); }
  }
  function archive() {
    if (archiving) return;
    setArchiving(true);
    void onArchive().finally(() => setArchiving(false));
  }
  const archiveItem: CardMenuItem = { id: "archive", label: "Archive account", disabled, busy: archiving, run: archive };
  const confirm = (id: string, itemLabel: string, question: string, confirmLabel: string, run: (done: () => void) => Promise<void>, blockedToo = false): CardMenuItem => ({
    id, label: itemLabel, disabled: disabled || blockedToo,
    panel: done => <ConfirmPanel question={question} confirmLabel={confirmLabel} disabled={disabled || blockedToo} onConfirm={() => run(done)} onCancel={done} />,
  });

  if (!manager) {
    return {
      items: current ? [] : [archiveItem],
      danger: current ? [] : [confirm("remove", "Remove account", removeAccountQuestion(label), "Confirm removal", done => whileRemoving(onForget).then(done))],
      footer: footer?.({ current, blocked: removing, unconfirmedCurrent: false }) ?? null,
    };
  }

  const { provider, busy, query, action, removeAccount, use, add, cancel, loginTarget } = manager;
  const nativeMatches = !query.isFetching && query.data?.nativeObservationId === accountId;
  const unconfirmedCurrent = current && !nativeMatches;
  const signingIn = loginTarget === accountId && (busy === "login" || busy === "cancelLogin");
  const switchingAlongside = clients && profile && !profile.needsLogin ? { clients, profile } : null;
  function startSwitch(profileId: string) {
    setClients(null);
    void use(profileId).then(running => setClients(running.length ? running : null)).catch(() => {});
  }
  function primaryAction(): ReactNode {
    if (signingIn) return <button className={button} disabled={busy === "cancelLogin"} onClick={() => void cancel()}>{busy === "cancelLogin" ? "Canceling…" : "Cancel sign-in"}</button>;
    if (signInAgain && profile) return <button className={button} disabled={disabled} onClick={() => void add(profile.id, accountId)}>Sign in again</button>;
    if (archiveInsteadOfUse) return <button className={button} disabled={disabled} aria-disabled={archiving || undefined} aria-busy={archiving || undefined} onClick={archive}>Archive account</button>;
    if (!profile) return <button className={button} disabled={disabled || unconfirmedCurrent} onClick={() => current ? void action("save").catch(() => {}) : void add(undefined, accountId)}>{current ? "Save account" : "Sign in"}</button>;
    if (current && !profile.pendingActivation) return null;
    return <button ref={useButton} className={button} disabled={disabled} onClick={() => profile.needsLogin ? void add(profile.id, accountId) : startSwitch(profile.id)}>{profile.needsLogin ? "Sign in" : "Use account"}</button>;
  }
  const items: CardMenuItem[] = [];
  if (profile) {
    items.push({ id: "category", label: "Edit category", disabled, panel: done => <CategoryPanel initial={profile.category ?? ""} disabled={disabled} onSave={category => action("category", profile.id, category)} done={done} /> });
    items.push({ id: "signInAgain", label: "Sign in again", disabled, run: () => void add(profile.id, accountId) });
  }
  if (current && profile) items.push(confirm("removeLogin", "Remove saved login", `Remove the saved login for ${label}? This account stays signed in.`, "Confirm removal",
    done => whileRemoving(() => action("remove", profile.id)).then(done)));
  if (!current) items.push(archiveItem);
  const danger = [current
    ? confirm("signOut", "Sign out", "Sign out of this account? The provider may also revoke its saved sign-ins.", "Confirm sign out", async done => {
      if (!nativeMatches || cachedAccountList(client, provider)?.nativeObservationId !== accountId) { done(); return; }
      await whileRemoving(() => action("signOut"));
      done();
    }, !nativeMatches)
    : confirm("remove", "Remove account", removeAccountQuestion(label), "Confirm removal", done => whileRemoving(() => removeAccount(profile?.id, onForget)).then(done))];
  return {
    items,
    danger,
    footer: <>
      {primaryAction()}
      {switchingAlongside
        ? <SwitchAlongsideConfirmation product={providerLabel(provider)} clients={switchingAlongside.clients} disabled={!!disabled}
          onSwitch={() => { setClients(null); void action("useAlongsideClients", switchingAlongside.profile.id).catch(() => {}); }}
          onCancel={() => { setClients(null); useButton.current?.focus(); }} />
        : footer?.({ current, blocked: !!disabled, unconfirmedCurrent })}
    </>,
  };
}

function ConfirmPanel({ question, confirmLabel, disabled, onConfirm, onCancel }: {
  question: string; confirmLabel: string; disabled: boolean; onConfirm: () => Promise<void>; onCancel: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  async function confirm() {
    setError(null);
    try { await onConfirm(); }
    catch (reason) { setError(parseInvokeError(reason).message); }
  }
  return <>
    <div role="group" aria-label="Confirm account action" className="text-[12px]">
      <p>{question}</p>
      <div className="flex gap-2"><button className={button} disabled={disabled} onClick={() => void confirm()}>{confirmLabel}</button><button className={button} onClick={onCancel}>Cancel</button></div>
    </div>
    {error && <p role="alert" className="text-[12px] text-[var(--trip)]">{error}</p>}
  </>;
}

function CategoryPanel({ initial, disabled, onSave, done }: { initial: string; disabled: boolean; onSave: (category: string) => Promise<void>; done: () => void }) {
  const [category, setCategory] = useState(initial);
  return <form role="group" aria-label="Edit account category" className="flex flex-wrap gap-2" onSubmit={event => { event.preventDefault(); void onSave(category).then(done).catch(() => {}); }}>
    <input aria-label="Category (optional)" placeholder="Category (optional)" maxLength={100} value={category} onChange={event => setCategory(event.target.value)} className="w-full min-w-0 rounded border border-[var(--hair)] bg-transparent px-2 py-1.5 text-[12px]" />
    <button className={button} disabled={disabled}>Save category</button><button type="button" className={button} onClick={done}>Cancel</button>
  </form>;
}

function SwitchAlongsideConfirmation({ product, clients, disabled, onSwitch, onCancel }: {
  product: string; clients: string[]; disabled: boolean; onSwitch: () => void; onCancel: () => void;
}) {
  return <div role="group" aria-label={`Confirm switching while ${product} is running`} className="flex w-full flex-col gap-2 text-[12px]"
    onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); onCancel(); } }}>
    <p className="m-0">{product} is still running in {clients.join(", ")}. Those sessions keep using the current account until you restart them. Don't sign out or sign in again from them: that can revoke saved logins.</p>
    <div className="flex gap-2"><button className={button} disabled={disabled} onClick={onSwitch}>Switch anyway</button><button autoFocus className={button} onClick={onCancel}>Cancel</button></div>
  </div>;
}
