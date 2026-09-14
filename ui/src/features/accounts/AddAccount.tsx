import { useEffect, useId, useRef, useState } from "react";
import { Plus } from "lucide-react";
import { ProviderIcon } from "$lib/ProviderIcon";
import { accountButton, useAccountControllers } from "./AccountManager";
import { AutomaticAccountSaving } from "./AccountPreferences";

export function AddAccount() {
  const controllers = useAccountControllers();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLButtonElement>("[role=group] button:not(:disabled)")?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  return <div className="relative" ref={root} onBlur={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) setOpen(false); }} onKeyDown={event => {
    if (event.key === "Escape") { setOpen(false); trigger.current?.focus(); }
  }}>
    <button ref={trigger} type="button" className={`${accountButton} inline-flex items-center gap-1.5`} aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(!open)}>
      <Plus className="size-3.5" aria-hidden="true" />Add account
    </button>
    {open && <div id={id} role="group" aria-label="Add account" className="absolute right-0 top-full z-20 mt-2 w-72 max-w-[calc(100vw-3rem)] rounded-lg border border-[var(--hair)] bg-[var(--plate)] p-2 shadow-lg">
      {(["claude", "codex"] as const).map(provider => <button key={provider} type="button" className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-[13px] hover:bg-[var(--wash)] focus-visible:outline focus-visible:outline-[var(--fill)] disabled:opacity-45" disabled={!!controllers[provider].busy || controllers[provider].query.isPending || controllers[provider].query.data?.recoveryRequired} onClick={() => {
        setOpen(false); trigger.current?.focus(); void controllers[provider].add();
      }}><span aria-hidden="true"><ProviderIcon provider={provider} className="size-4" /></span>{provider === "claude" ? "Claude" : "Codex"}</button>)}
      <div className="mt-2 border-t border-[var(--hair)] px-3 pt-3 pb-2"><AutomaticAccountSaving /></div>
    </div>}
  </div>;
}
