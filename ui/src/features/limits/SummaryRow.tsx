import { useId, type ReactNode } from "react";

/**
 * One more row under a card's windows for an account figure that is not a quota: a small-caps
 * label, an optional note under it, and the value on the right. The value is the label's definition,
 * so it is announced with its name.
 */
export function SummaryRow({ label, value, note }: { label: string; value: ReactNode; note?: string }) {
  const labelId = useId();
  return (
    <dl className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <dt id={labelId} className="text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
          {label}
        </dt>
        {note ? <dd className="font-mono text-[11px] leading-snug text-[var(--mute)]">{note}</dd> : null}
      </div>
      <dd aria-labelledby={labelId} className="shrink-0 text-right font-mono text-[12px] tabular-nums">
        {value}
      </dd>
    </dl>
  );
}
