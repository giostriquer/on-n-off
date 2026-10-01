import { useEffect, useId, useRef, useState, type ReactNode, type RefCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { AccountsReading, SavedProfile } from "$lib/accountTypes";
import { parseInvokeError } from "$lib/error";
import { accountButton as button, useAccountManagement } from "./AccountManager";

export type AccountFooterState = { current: boolean; blocked: boolean; unconfirmedCurrent: boolean };

export function removeAccountQuestion(label: string): string {
  return `Remove ${label} from on-n-off? You will need to sign in to add it again.`;
}

type Panel = "menu" | "category" | "extra" | Confirmation;
type Confirmation = "remove" | "removeLogin" | "signOut";

export function AccountCardActions({ accountId, label, current, profile, onForget, onArchive, archiveInsteadOfUse = false, menuButtonRef, extraAction, header, footer, children }: {
  accountId: string; label: string; current: boolean; profile?: SavedProfile;
  onForget: () => Promise<void>;
  onArchive: () => Promise<void>;
  archiveInsteadOfUse?: boolean;
  menuButtonRef?: RefCallback<HTMLButtonElement>;
  extraAction?: { label: string; render: (close: () => void) => ReactNode };
  header: (menu: ReactNode) => ReactNode;
  footer?: (state: AccountFooterState) => ReactNode;
  children?: ReactNode;
}) {
  const manager = useAccountManagement();
  const client = useQueryClient();
  const [panel, setPanel] = useState<Panel | null>(null);
  const [category, setCategory] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const [clients, setClients] = useState<string[] | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const useButton = useRef<HTMLButtonElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const open = panel !== null;
  const confirmation = panel === "remove" || panel === "removeLogin" || panel === "signOut" ? panel : null;
  function dismiss() { setPanel(null); setError(null); }
  function close() { dismiss(); trigger.current?.focus(); }
  function complete() {
    if (root.current?.contains(document.activeElement)) close();
    else dismiss();
  }
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>("[role=group]:not([hidden]) input, [role=group]:not([hidden]) button:not(:disabled)")?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) dismiss();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [panel]);
  if (!manager) return <>{header(null)}{children}</>;
  const { provider, busy, blocked, query, action, removeAccount, use, add, cancel, loginTarget } = manager;
  const nativeMatches = !query.isFetching && query.data?.nativeObservationId === accountId;
  const unconfirmedCurrent = current && !nativeMatches;
  const signingIn = loginTarget === accountId && (busy === "login" || busy === "cancelLogin");
  const disabled = blocked || removing;
  const switchingAlongside = clients && profile && !profile.needsLogin ? { clients, profile } : null;
  function startSwitch(profileId: string) {
    setClients(null);
    void use(profileId).then(running => setClients(running.length ? running : null)).catch(() => {});
  }
  async function confirm() {
    if (confirmation === "signOut" && (!current || !nativeMatches || client.getQueryData<AccountsReading>(["accounts", provider])?.nativeObservationId !== accountId)) { setPanel(null); return; }
    setError(null); setRemoving(true);
    try {
      if (confirmation === "signOut") await action("signOut");
      else if (confirmation === "removeLogin" && profile) await action("remove", profile.id);
      else await removeAccount(profile?.id, onForget);
      complete();
    } catch (error) { setError(parseInvokeError(error).message); }
    finally { setRemoving(false); }
  }
  function archive() {
    if (archiving) return;
    setArchiving(true);
    void onArchive().finally(() => setArchiving(false));
  }
  function primaryAction(): ReactNode {
    if (signingIn) return <button className={button} disabled={busy === "cancelLogin"} onClick={() => void cancel()}>{busy === "cancelLogin" ? "Canceling…" : "Cancel sign-in"}</button>;
    if (archiveInsteadOfUse) return <button className={button} disabled={disabled} aria-disabled={archiving || undefined} aria-busy={archiving || undefined} onClick={archive}>Archive account</button>;
    if (!profile) return <button className={button} disabled={disabled || unconfirmedCurrent} onClick={() => current ? void action("save").catch(() => {}) : void add(undefined, accountId)}>{current ? "Save account" : "Sign in"}</button>;
    if (current && !profile.pendingActivation) return null;
    return <button ref={useButton} className={button} disabled={disabled} onClick={() => profile.needsLogin ? void add(profile.id, accountId) : startSwitch(profile.id)}>{profile.needsLogin ? "Sign in" : "Use account"}</button>;
  }
  const menu = <div className="relative shrink-0" ref={root} onBlur={event => {
    if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) dismiss();
  }} onKeyDown={event => {
    if (event.key === "Escape") { event.stopPropagation(); close(); }
  }}>
    <button ref={node => { trigger.current = node; menuButtonRef?.(node); }} type="button" aria-label={`More actions for ${label}`} aria-expanded={open} aria-controls={open ? id : undefined}
      className="flex size-6 items-center justify-center rounded-md text-[var(--mute)] hover:bg-[var(--wash)] hover:text-[var(--silkscreen)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)]"
      onClick={() => open ? close() : setPanel("menu")}>•••</button>
    <div id={id} hidden={!open} className="absolute right-0 top-full z-20 mt-2 w-64 max-w-[calc(100vw-3rem)] rounded-lg border border-[var(--hair)] bg-[var(--plate)] p-2 shadow-lg">
    <div hidden={panel !== "menu"} role="group" aria-label={`Actions for ${label}`} className={panel === "menu" ? "flex flex-col gap-1" : "hidden"}>
      {profile && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => { setCategory(profile.category ?? ""); setPanel("category"); }}>Edit category</button>}
      {profile && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => { close(); void add(profile.id, accountId); }}>Sign in again</button>}
      {current && profile && <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => setPanel("removeLogin")}>Remove saved login</button>}
      {!current && <button className={`${button} border-transparent text-left`} disabled={disabled} aria-disabled={archiving || undefined} aria-busy={archiving || undefined} onClick={() => { close(); archive(); }}>Archive account</button>}
      {extraAction && <button className={`${button} border-transparent text-left`} onClick={() => setPanel("extra")}>{extraAction.label}</button>}
      {current ? <button className={`${button} border-transparent text-left`} disabled={disabled || !nativeMatches} onClick={() => setPanel("signOut")}>Sign out</button>
        : <button className={`${button} border-transparent text-left`} disabled={disabled} onClick={() => setPanel("remove")}>Remove account</button>}
    </div>
    {panel === "category" && profile && <form role="group" aria-label="Edit account category" className="flex flex-wrap gap-2" onSubmit={event => { event.preventDefault(); void action("category", profile.id, category).then(complete).catch(() => {}); }}>
      <input aria-label="Category (optional)" placeholder="Category (optional)" maxLength={100} value={category} onChange={event => setCategory(event.target.value)} className="w-full min-w-0 rounded border border-[var(--hair)] bg-transparent px-2 py-1.5 text-[12px]" />
      <button className={button} disabled={disabled}>Save category</button><button type="button" className={button} onClick={close}>Cancel</button>
    </form>}
    {panel === "extra" && extraAction?.render(close)}
    {confirmation && <div role="group" aria-label="Confirm account action" className="text-[12px]">
      <p>{confirmation === "remove" ? removeAccountQuestion(label) : confirmation === "removeLogin" ? `Remove the saved login for ${label}? This account stays signed in.` : "Sign out of this account? The provider may also revoke its saved sign-ins."}</p>
      <div className="flex gap-2"><button className={button} disabled={disabled || (confirmation === "signOut" && (!current || !nativeMatches))} onClick={() => void confirm()}>{confirmation === "signOut" ? "Confirm sign out" : "Confirm removal"}</button><button className={button} onClick={close}>Cancel</button></div>
    </div>}
    {error && <p role="alert" className="text-[12px] text-[var(--trip)]">{error}</p>}
    </div>
  </div>;
  return <>
    {header(menu)}
    {children}
    <footer className="flex flex-wrap items-center gap-2 border-t border-[var(--hair)] px-3.5 py-2.5 empty:hidden">
      {primaryAction()}
      {switchingAlongside
        ? <SwitchAlongsideConfirmation product={provider === "codex" ? "Codex" : "Claude"} clients={switchingAlongside.clients} disabled={!!disabled}
          onSwitch={() => { setClients(null); void action("useAlongsideClients", switchingAlongside.profile.id).catch(() => {}); }}
          onCancel={() => { setClients(null); useButton.current?.focus(); }} />
        : footer?.({ current, blocked: !!disabled, unconfirmedCurrent })}
    </footer>
  </>;
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
