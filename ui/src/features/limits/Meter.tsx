import { useId } from "react";
import { usageFillStyle } from "$lib/limitsFormat";
import type { AgentId } from "$lib/types";

/** A quota bar: the provider's accent, hardening toward red as it fills. */
export function Meter({
  label,
  percent,
  provider,
  className,
  describedBy,
}: {
  label: string;
  percent: number;
  provider: AgentId;
  className: string;
  describedBy?: string;
}) {
  return (
    <div
      className={`overflow-hidden rounded-sm bg-[var(--well)] ${className}`}
      role="meter"
      aria-label={label}
      aria-describedby={describedBy}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(percent)}
    >
      <div
        className="h-full rounded-sm transition-[width]"
        style={{ width: `${percent}%`, ...usageFillStyle(provider, percent) }}
      />
    </div>
  );
}

/**
 * A compact meter row: small-caps label with its note underneath (never truncated), bar and figure
 * on the right — the same idiom as the Overview's list rows. The bar names the note as its
 * description, so it is announced with what it measures.
 */
export function MeterRow({
  label,
  note,
  percent,
  text,
  color,
  provider,
}: {
  label: string;
  note: string;
  percent: number;
  text: string;
  color: string | undefined;
  provider: AgentId;
}) {
  const noteId = useId();
  return (
    <div className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{label}</span>
        <span id={noteId} className="font-mono text-[11px] leading-snug text-[var(--mute)]">{note}</span>
      </div>
      <Meter label={label} percent={percent} provider={provider} className="h-1 w-24 shrink-0" describedBy={noteId} />
      <span className="w-11 shrink-0 text-right font-mono text-[12px]" style={{ color }}>
        {text}
      </span>
    </div>
  );
}
