import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { AccountsReading, SavedProfile } from "$lib/accountTypes";
import { parseInvokeError } from "$lib/error";
import { AccountBilling } from "./AccountBilling";
import { accountButton as button, useAccountManagement } from "./AccountManager";

export function AccountCardActions({ accountId, label, current, profile, onForget, header, children }: {
  accountId: string; label: string; current: boolean; profile?: SavedProfile;
  onForget?: (id: string) => Promise<void>;
  header: (menu: ReactNode) => ReactNode;
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
  const root = useRef<HTMLDivElement>(null);
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
  const { provider, busy, query, action, add, cancel, loginTarget } = manager;
  const nativeMatches = !query.isFetching && query.data?.nativeObservationId === accountId;
  const signingIn = loginTarget === accountId && (busy === "login" || busy === "cancelLogin");
  const disabled = !!busy || removing || query.isPending || !!query.error || query.data?.recoveryRequired;
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
      {signingIn ? <button className={button} disabled={busy === "cancelLogin"} onClick={() => void cancel()}>{busy === "cancelLogin" ? "Canceling…" : "Cancel sign-in"}</button> : profile ? (!current || profile.pendingActivation) && <button className={button} disabled={disabled} onClick={() => profile.needsLogin ? void add(profile.id, accountId) : void action("use", profile.id).catch(() => {})}>{profile.needsLogin ? "Sign in" : "Use account"}</button>
        : <button className={button} disabled={disabled || (current && !nativeMatches)} onClick={() => current ? void action("save").catch(() => {}) : void add(undefined, accountId)}>{current ? "Save account" : "Sign in"}</button>}
      {provider === "codex" && <AccountBilling accountId={accountId} disabled={disabled} showDate={false} />}
    </footer>
  </>;
}
