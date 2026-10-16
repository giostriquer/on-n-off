import { useEffect, useId, useRef, useState, type ReactNode, type RefCallback } from "react";
import { accountButton as button } from "@/features/accounts/AccountManager";

export type CardMenuItem = {
  id: string;
  label: string;
  disabled?: boolean;
  busy?: boolean;
} & ({ run: () => void; panel?: never } | { panel: (done: () => void) => ReactNode; run?: never });

export function CardMenu({ label, items, buttonRef }: {
  label: string;
  items: CardMenuItem[];
  buttonRef?: RefCallback<HTMLButtonElement>;
}) {
  const [panel, setPanel] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const open = panel !== null;
  function dismiss() { setPanel(null); }
  function close() { dismiss(); trigger.current?.focus(); }
  function done() {
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
  const opened = items.find(item => item.id === panel);
  return <div className="relative shrink-0" ref={root} onBlur={event => {
    if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) dismiss();
  }} onKeyDown={event => {
    if (event.key === "Escape") { event.stopPropagation(); close(); }
  }}>
    <button ref={node => { trigger.current = node; buttonRef?.(node); }} type="button" aria-label={`More actions for ${label}`} aria-expanded={open} aria-controls={open ? id : undefined}
      className="flex size-6 items-center justify-center rounded-md text-[var(--mute)] hover:bg-[var(--wash)] hover:text-[var(--silkscreen)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)]"
      onClick={() => open ? close() : setPanel("menu")}>•••</button>
    <div id={id} hidden={!open} className="absolute right-0 top-full z-20 mt-2 w-64 max-w-[calc(100vw-3rem)] rounded-lg border border-[var(--hair)] bg-[var(--plate)] p-2 shadow-lg">
      <div hidden={panel !== "menu"} role="group" aria-label={`Actions for ${label}`} className={panel === "menu" ? "flex flex-col gap-1" : "hidden"}>
        {items.map(item => <button key={item.id} type="button" className={`${button} border-transparent text-left`} disabled={item.disabled}
          aria-disabled={item.busy || undefined} aria-busy={item.busy || undefined}
          onClick={() => {
            if (item.run) { close(); item.run(); }
            else setPanel(item.id);
          }}>{item.label}</button>)}
      </div>
      {opened?.panel?.(done)}
    </div>
  </div>;
}
