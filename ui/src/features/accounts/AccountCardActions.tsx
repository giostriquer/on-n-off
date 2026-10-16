import { useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { confirmItem, type ActionMenuItem } from "@/components/ActionMenu";
import { actionButton as button } from "@/components/actionButton";
import type { SavedProfile } from "$lib/accountTypes";
import { providerLabel } from "$lib/providers";
import { cachedAccountList } from "$lib/sharedReads/accounts";
import { useAccountManagement } from "./AccountManager";

export type AccountFooterState = { current: boolean; blocked: boolean; unconfirmedCurrent: boolean };

export type AccountCardInput = {
  accountId: string;
  label: string;
  current: boolean;
  profile?: SavedProfile;
  archive: { run: () => void; busy: boolean };
  blocked?: boolean;
  archiveInsteadOfUse?: boolean;
  signInAgain?: boolean;
  footer?: (state: AccountFooterState) => ReactNode;
};

export type AccountCardActions = { items: ActionMenuItem[]; danger: ActionMenuItem[]; footer: ReactNode };

const NO_ACTIONS: AccountCardActions = { items: [], danger: [], footer: null };

export function useAccountCardActions(account: AccountCardInput | null): AccountCardActions {
  const manager = useAccountManagement();
  const client = useQueryClient();
  const [clients, setClients] = useState<string[] | null>(null);
  const useButton = useRef<HTMLButtonElement>(null);
  if (!manager || !account) return NO_ACTIONS;
  const { accountId, label, current, profile, archive, archiveInsteadOfUse = false, signInAgain = false, footer } = account;
  const { provider, busy, query, action, use, add, cancel, loginTarget } = manager;
  const disabled = manager.blocked || !!account.blocked;
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
    if (archiveInsteadOfUse) return <button className={button} disabled={disabled} aria-disabled={archive.busy || undefined} aria-busy={archive.busy || undefined} onClick={archive.run}>Archive account</button>;
    if (!profile) return <button className={button} disabled={disabled || unconfirmedCurrent} onClick={() => current ? void action("save").catch(() => {}) : void add(undefined, accountId)}>{current ? "Save account" : "Sign in"}</button>;
    if (current && !profile.pendingActivation) return null;
    return <button ref={useButton} className={button} disabled={disabled} onClick={() => profile.needsLogin ? void add(profile.id, accountId) : startSwitch(profile.id)}>{profile.needsLogin ? "Sign in" : "Use account"}</button>;
  }
  const items: ActionMenuItem[] = [];
  if (profile) {
    items.push({ id: "signInAgain", label: "Sign in again", disabled, run: () => void add(profile.id, accountId) });
    if (current) items.push(confirmItem({
      id: "removeLogin", label: "Remove saved login", question: `Remove the saved login for ${label}? This account stays signed in.`, confirmLabel: "Confirm removal", disabled,
      onConfirm: async done => { await action("remove", profile.id); done(); },
    }));
  }
  const danger = current ? [confirmItem({
    id: "signOut", label: "Sign out", question: "Sign out of this account? The provider may also revoke its saved sign-ins.", confirmLabel: "Confirm sign out", disabled: disabled || !nativeMatches,
    onConfirm: async done => {
      if (nativeMatches && cachedAccountList(client, provider)?.nativeObservationId === accountId) await action("signOut");
      done();
    },
  })] : [];
  return {
    items,
    danger,
    footer: <>
      {primaryAction()}
      {switchingAlongside
        ? <SwitchAlongsideConfirmation product={providerLabel(provider)} clients={switchingAlongside.clients} disabled={disabled}
          onSwitch={() => { setClients(null); void action("useAlongsideClients", switchingAlongside.profile.id).catch(() => {}); }}
          onCancel={() => { setClients(null); useButton.current?.focus(); }} />
        : footer?.({ current, blocked: disabled, unconfirmedCurrent })}
    </>,
  };
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
