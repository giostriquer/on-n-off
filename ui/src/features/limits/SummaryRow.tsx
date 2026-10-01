import { useId, type ReactNode } from "react";

export type NoteLines = { label: string; lines: readonly string[] };

export function SummaryRow({ label, value, note }: {
  label: string;
  value: ReactNode;
  note?: string | NoteLines;
}) {
  const labelId = useId();
  return (
    <dl className="border-t border-[var(--hair)] px-3.5 py-2">
      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-2.5 gap-y-1">
        <dt id={labelId} className="col-start-1 row-start-1 text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">
          {label}
        </dt>
        <dd aria-labelledby={labelId} className="col-start-2 row-span-2 row-start-1 text-right font-mono text-[12px] tabular-nums">
          {value}
        </dd>
        {note ? (
          <dd className="col-start-1 row-start-2 font-mono text-[11px] leading-snug text-[var(--mute)]">
            {typeof note === "string" ? note : (
              <ul aria-label={note.label} className="m-0 list-none p-0">
                {note.lines.map((line, index) => <li key={index}>{line}</li>)}
              </ul>
            )}
          </dd>
        ) : null}
      </div>
    </dl>
  );
}
