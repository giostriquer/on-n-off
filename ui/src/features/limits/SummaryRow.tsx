import { useId, type ReactNode } from "react";

/**
 * One more row under a card's windows for an account figure that is not a quota: a small-caps
 * label, an optional note under it, and the value on the right. The value is the label's definition,
 * so it is announced with its name. A figure made of parts can list them under the note, one per
 * line, as a list named `listLabel`.
 */
export function SummaryRow({ label, value, note, list, listLabel }: {
  label: string;
  value: ReactNode;
  note?: string;
  list?: string[] | null;
  listLabel?: string;
}) {
  const labelId = useId();
  // One name–value group: the label, its value, then the note and any list, laid out so the note sits under the label.
  return (
    <dl className="border-t border-[var(--hair)] px-3.5 py-2">
      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-2.5 gap-y-1">
        <dt id={labelId} className="col-start-1 row-start-1 text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
          {label}
        </dt>
        <dd aria-labelledby={labelId} className="col-start-2 row-span-2 row-start-1 text-right font-mono text-[12px] tabular-nums">
          {value}
        </dd>
        {note ? <dd className="col-start-1 row-start-2 font-mono text-[11px] leading-snug text-[var(--mute)]">{note}</dd> : null}
        {list?.length ? (
          <dd className={`col-span-2 col-start-1 ${note ? "row-start-3" : "row-start-2"}`}>
            <ul aria-label={listLabel} className="m-0 list-none p-0 font-mono text-[11px] leading-snug text-[var(--mute)]">
              {list.map((line, index) => <li key={index}>{line}</li>)}
            </ul>
          </dd>
        ) : null}
      </div>
    </dl>
  );
}
