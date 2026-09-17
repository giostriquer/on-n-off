import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as api from "$lib/api";
import type { AccountAction, AccountProvider } from "$lib/accountTypes";
import { parseInvokeError } from "$lib/error";
import { useSharedRead } from "$lib/useSharedRead";

export const accountButton = "rounded-md border border-[var(--hair)] px-2.5 py-1.5 text-[11px] hover:bg-[var(--wash)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)] disabled:opacity-45";
function useController(provider: AccountProvider) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const operation = useRef<string | null>(null);
  const [loginTarget, setLoginTarget] = useState<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (operation.current) void api.cancelAccountLogin(operation.current).catch(() => {});
    };
  }, []);
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["accounts", provider], queryFn: () => api.readAccounts(provider), retry: false });
  useSharedRead("accounts");
  async function refresh() {
    await Promise.all([client.invalidateQueries({ queryKey: ["accounts"] }), client.invalidateQueries({ queryKey: ["limits", provider] }), client.invalidateQueries({ queryKey: ["subscription", "codex"] })]);
  }
  async function action(action: AccountAction, id?: string, value?: string, after?: () => Promise<void>) {
    setBusy(action); setError(null);
    try { await api.accountAction(provider, action, id, value); await after?.(); }
    catch (error) { setError(parseInvokeError(error).message); throw error; }
    finally { try { await refresh(); } finally { setBusy(null); } }
  }
  /** A failed scan is not a reason to refuse: the switch itself checks again and reports it. */
  async function activationBlockers() {
    try { return await api.readAccountActivationBlockers(provider); } catch { return []; }
  }
  async function add(profileId?: string, accountId?: string) {
    if (operation.current) return;
    const id = crypto.randomUUID(); operation.current = id;
    setLoginTarget(accountId ?? null);
    setBusy("login"); setError(null);
    try { await api.addAccount(provider, id, profileId); }
    catch (error) { if (mounted.current) setError(parseInvokeError(error).message); }
    finally {
      operation.current = null;
      if (mounted.current) {
        setLoginTarget(null); setBusy("refresh");
        try { await refresh(); } finally { if (mounted.current) setBusy(null); }
      }
    }
  }
  async function cancel() {
    const id = operation.current;
    if (!id || busy === "cancelLogin") return;
    setBusy("cancelLogin");
    try { await api.cancelAccountLogin(id); }
    catch (error) {
      if (mounted.current && operation.current === id) {
        setBusy("login"); setError(parseInvokeError(error).message);
      }
    }
  }
  return { provider, query, busy, error, action, activationBlockers, add, cancel, loginTarget };
}
const Controllers = createContext<Record<AccountProvider, ReturnType<typeof useController>> | null>(null);
export function AccountControllers({ children }: { children: ReactNode }) {
  const claude = useController("claude");
  const codex = useController("codex");
  return <Controllers.Provider value={{ claude, codex }}>{children}</Controllers.Provider>;
}
export function useAccountControllers() {
  const controllers = useContext(Controllers);
  if (!controllers) throw new Error("AccountControllers is required");
  return controllers;
}
const Context = createContext<ReturnType<typeof useController> | null>(null);
export function useAccountManagement() { return useContext(Context); }

/** Shares operation state; accounts themselves are rendered only by their Limits cards. */
export function AccountManager({ provider, children }: { provider: AccountProvider; children: ReactNode }) {
  const controller = useAccountControllers()[provider];
  const { query, busy, error, action, cancel, loginTarget } = controller;
  return <Context.Provider value={controller}>
    {(busy === "login" || busy === "cancelLogin") && !loginTarget && <div role="status" className="flex flex-wrap items-center gap-2 text-[12px] text-[var(--mute)]">Finish signing in in your browser.<button className={accountButton} disabled={busy === "cancelLogin"} onClick={() => void cancel()}>{busy === "cancelLogin" ? "Canceling…" : "Cancel sign-in"}</button></div>}
    {query.isPending && <p role="status" className="m-0 text-[12px] text-[var(--mute)]">Loading accounts…</p>}
    {busy && busy !== "login" && busy !== "cancelLogin" && <p role="status" className="m-0 text-[12px] text-[var(--mute)]">Updating account…</p>}
    {(error || query.error || query.data?.notice) && <div role="alert" className="flex items-center gap-2 text-[12px] text-[var(--trip)]"><span className="min-w-0 flex-1">{error ?? (query.error ? parseInvokeError(query.error).message : query.data?.notice)}</span>{query.error && <button className={`${accountButton} shrink-0 text-[var(--silkscreen)]`} aria-label="Retry account access" disabled={!!busy} onClick={() => void action("unlock").catch(() => {})}>Retry</button>}</div>}
    {query.data?.recoveryRequired && <div className="rounded-md border border-[var(--hair)] p-3 text-[12px]"><p>An interrupted account change needs recovery.</p><button className={accountButton} disabled={!!busy} onClick={() => void action("recover").catch(() => {})}>Recover account</button></div>}
    {children}
  </Context.Provider>;
}
