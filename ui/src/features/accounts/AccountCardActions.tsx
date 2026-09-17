import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { AccountsReading, SavedProfile } from "$lib/accountTypes";
import { parseInvokeError } from "$lib/error";
import { accountButton as button, useAccountManagement } from "./AccountManager";

/**
 * What more account actions beside the primary one can act on. `current` is the card's own notion of
 * the signed-in account. `blocked` holds while the account controls cannot act: an operation is
 * running, accounts are loading or failed to load, or recovery is required. `unconfirmedCurrent`
 * marks a current card whose native login is not confirmed as this account. Each action decides
 * which of them stop it.
 */
export type AccountFooterState = { current: boolean; blocked: boolean; unconfirmedCurrent: boolean };

export function AccountCardActions({ accountId, label, current, profile, onForget, header, footer, children }: {
  accountId: string; label: string; current: boolean; profile?: SavedProfile;
  onForget?: (id: string) => Promise<void>;
  header: (menu: ReactNode) => ReactNode;
  /** More account actions beside the primary one; see `AccountFooterState`. */
  footer?: (state: AccountFooterState) => ReactNode;
  children?: ReactNode;
}) {
  const manager = useAccountManagement();
  const client = useQueryClient();
  const [menuOpen, setMenuOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [category, setCategory] = useState("");
  const [confirmation, setConfirmation] = useState<"remove" | "removeLogin" | "signOut" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const [clients, setClients] = useState<string[] | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const useButton = useRef<HTMLButtonElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const open = menuOpen || editing || !!confirmation;
  function dismiss() { setMenuOpen(false); setEditing(false); setConfirmation(null); setError(null); }
  function close() { dismiss(); trigger.current?.focus(); }
  function complete() {
    if (root.current?.contains(document.activeElement)) close();
    else dismiss();
  }
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>("[role=group] input, [role=group] button:not(:disabled)")?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) dismiss();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open, editing, confirmation]);
  if (!manager) return <>{header(null)}{children}</>;
  const { provider, busy, query, action, use, add, cancel, loginTarget } = manager;
  const nativeMatches = !query.isFetching && query.data?.nativeObservationId === accountId;
  // A current card whose native login is not confirmed as this account must not act on it.
  const unconfirmedCurrent = current && !nativeMatches;
  const signingIn = loginTarget === accountId && (busy === "login" || busy === "cancelLogin");
  const disabled = !!busy || removing || query.isPending || !!query.error || query.data?.recoveryRequired;
  const switchingAlongside = clients && profile && !profile.needsLogin ? { clients, profile } : null;
  function startSwitch(profileId: string) {
    setClients(null);
    void use(profileId).then(running => setClients(running.length ? running : null)).catch(() => {});
  }
  async function confirm() {
    if (confirmation === "signOut" && (!current || !nativeMatches || client.getQueryData<AccountsReading>(["accounts", provider])?.nativeObservationId !== accountId)) { setConfirmation(null); return; }
    setError(null); setRemoving(true);
    try {
      if (confirmation === "signOut") await action("signOut");
      else if (confirmation === "removeLogin" && profile) await action("remove", profile.id);
      else {
        if (profile) await action("remove", profile.id, undefined, () => onForget?.(accountId) ?? Promise.resolve());
        else await onForget?.(accountId);
      }
      complete();
    } catch (error) { setError(parseInvokeError(error).message); }
    finally { setRemoving(false); }
  }
  const menu = <div className="relative shrink-0" ref={root} onBlur={event => {
    if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) dismiss();
  }} onKeyDown={event => {
    if (event.key === "Escape") { event.stopPropagation(); close(); }
  }}>
    <button ref={trigger} type="button" aria-label={`More actions for ${label}`} aria-expanded={open} aria-controls={open ? id : undefined}
      className="flex size-6 items-center justify-center rounded-md text-[var(--mute)] hover:bg-[var(--wash)] hover:text-[var(--silkscreen)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)]"
      onClick={() => open ? close() : setMenuOpen(true)}>•••</button>
    {open && <div id={id} className="absolute right-0 top-full z-20 mt-2 w-64 max-w-[calc(100vw-3rem)] rounded-lg border border-[var(--hair)] bg-[var(--plate)] p-2 shadow-lg">
    {menuOpen && <div role="group" aria-label={`Actions for ${label}`} className="flex flex-col gap-1">
      {profile && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => { setMenuOpen(false); setCategory(profile.category ?? ""); setEditing(true); }}>Edit category</button>}
      {profile && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => { close(); void add(profile.id, accountId); }}>Sign in again</button>}
      {current && profile && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => { setMenuOpen(false); setConfirmation("removeLogin"); }}>Remove saved login</button>}
      {current ? <button className={`${button} border-transparent text-left`} disabled={disabled || !nativeMatches} onClick={() => { setMenuOpen(false); setConfirmation("signOut"); }}>Sign out</button>
        : onForget && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => { setMenuOpen(false); setConfirmation("remove"); }}>Remove account</button>}
    </div>}
    {editing && profile && <form role="group" aria-label="Edit account category" className="flex flex-wrap gap-2" onSubmit={event => { event.preventDefault(); void action("category", profile.id, category).then(complete).catch(() => {}); }}>
      <input aria-label="Category (optional)" placeholder="Category (optional)" maxLength={100} value={category} onChange={event => setCategory(event.target.value)} className="w-full min-w-0 rounded border border-[var(--hair)] bg-transparent px-2 py-1.5 text-[12px]" />
      <button className={button} disabled={disabled}>Save category</button><button type="button" className={button} onClick={close}>Cancel</button>
    </form>}
    {confirmation && <div role="group" aria-label="Confirm account action" className="text-[12px]">
      <p>{confirmation === "remove" ? `Remove ${label} from on-n-off? You will need to sign in to add it again.` : confirmation === "removeLogin" ? `Remove the saved login for ${label}? This account stays signed in.` : "Sign out of this account? The provider may also revoke its saved sign-ins."}</p>
      <div className="flex gap-2"><button className={button} disabled={disabled || (confirmation === "signOut" && (!current || !nativeMatches))} onClick={() => void confirm()}>{confirmation === "signOut" ? "Confirm sign out" : "Confirm removal"}</button><button className={button} onClick={close}>Cancel</button></div>
    </div>}
    {error && <p role="alert" className="text-[12px] text-[var(--trip)]">{error}</p>}
    </div>}
  </div>;
  return <>
    {header(menu)}
    {children}
    <footer className="flex flex-wrap items-center gap-2 border-t border-[var(--hair)] px-3.5 py-2.5 empty:hidden">
      {signingIn ? <button className={button} disabled={busy === "cancelLogin"} onClick={() => void cancel()}>{busy === "cancelLogin" ? "Canceling…" : "Cancel sign-in"}</button> : profile ? (!current || profile.pendingActivation) && <button ref={useButton} className={button} disabled={disabled} onClick={() => profile.needsLogin ? void add(profile.id, accountId) : startSwitch(profile.id)}>{profile.needsLogin ? "Sign in" : "Use account"}</button>
        : <button className={button} disabled={disabled || unconfirmedCurrent} onClick={() => current ? void action("save").catch(() => {}) : void add(undefined, accountId)}>{current ? "Save account" : "Sign in"}</button>}
      {switchingAlongside
        ? <SwitchAlongsideConfirmation product={provider === "codex" ? "Codex" : "Claude"} clients={switchingAlongside.clients} disabled={!!disabled}
          onSwitch={() => { setClients(null); void action("useAlongsideClients", switchingAlongside.profile.id).catch(() => {}); }}
          onCancel={() => { setClients(null); useButton.current?.focus(); }} />
        : footer?.({ current, blocked: !!disabled, unconfirmedCurrent })}
    </footer>
  </>;
}

/** Asks before a switch that provider clients still running would not follow. */
function SwitchAlongsideConfirmation({ product, clients, disabled, onSwitch, onCancel }: {
  product: string; clients: string[]; disabled: boolean; onSwitch: () => void; onCancel: () => void;
}) {
  const keep = useRef<HTMLButtonElement>(null);
  useEffect(() => { keep.current?.focus(); }, []);
  return <div role="group" aria-label={`Confirm switching while ${product} is running`} className="flex w-full flex-col gap-2 text-[12px]"
    onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); onCancel(); } }}>
    <p className="m-0">{product} is still running in {clients.join(", ")}. Those sessions keep using the current account until you restart them. Don't sign out or sign in again from them: that can revoke saved logins.</p>
    <div className="flex gap-2"><button className={button} disabled={disabled} onClick={onSwitch}>Switch anyway</button><button ref={keep} className={button} onClick={onCancel}>Cancel</button></div>
  </div>;
}
