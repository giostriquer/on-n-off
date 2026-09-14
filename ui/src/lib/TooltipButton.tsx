import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** A focusable tooltip trigger; the popup stays within the viewport and survives pointer travel. */
export function TooltipButton({ children, tooltip, label, className }: {
  children: ReactNode; tooltip: ReactNode; label: string; className?: string;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const focused = useRef(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{left: number; top: number} | null>(null);
  function clearTimer() {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
  }
  function show() { clearTimer(); setOpen(true); }
  function hide() { clearTimer(); setOpen(false); }
  function leave() {
    clearTimer();
    if (!focused.current) timer.current = setTimeout(() => setOpen(false), 150);
  }
  useEffect(() => () => { if (timer.current !== null) clearTimeout(timer.current); }, []);
  useLayoutEffect(() => {
    if (!open || !trigger.current || !popup.current) { setPosition(null); return; }
    const anchor = trigger.current.getBoundingClientRect();
    const tip = popup.current.getBoundingClientRect();
    const margin = 8;
    const left = Math.max(margin, Math.min(anchor.right - tip.width, window.innerWidth - tip.width - margin));
    const below = anchor.bottom + margin;
    const top = below + tip.height <= window.innerHeight - margin ? below : Math.max(margin, anchor.top - tip.height - margin);
    setPosition({left, top});
  }, [open, tooltip]);
  useEffect(() => {
    if (!open) return;
    const dismiss = () => { clearTimer(); setOpen(false); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") dismiss(); };
    document.addEventListener("keydown", key);
    window.addEventListener("resize", dismiss);
    window.addEventListener("scroll", dismiss, true);
    return () => {
      document.removeEventListener("keydown", key);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("scroll", dismiss, true);
    };
  }, [open]);
  return <>
    <button ref={trigger} type="button" title="" aria-label={label} aria-describedby={open ? id : undefined} className={className}
      onPointerEnter={() => { clearTimer(); timer.current = setTimeout(show, 200); }} onPointerLeave={leave}
      onFocus={() => { focused.current = true; show(); }} onBlur={() => { focused.current = false; hide(); }} onClick={show}>
      {children}
    </button>
    {open && createPortal(<div ref={popup} id={id} role="tooltip" onPointerEnter={clearTimer} onPointerLeave={leave}
      className="fixed z-[100] max-w-[min(21rem,calc(100vw-16px))] rounded-lg border border-[var(--hair)] bg-[var(--plate)] px-3 py-2 text-[12px] leading-relaxed text-[var(--silkscreen)] shadow-xl"
      style={{left: position?.left ?? 0, top: position?.top ?? 0, visibility: position ? "visible" : "hidden"}}>
      {tooltip}
    </div>, document.body)}
  </>;
}
