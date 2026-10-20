import { useId } from "react";
import type { AgentId } from "$lib/types";
import { usageFillStyle, usageTextColor } from "$lib/windowAt";
import type { Gauge, LimitWindowPresentation } from "./limitPresentation";

const TRACK = { card: "rounded-sm bg-[var(--well)]", popover: "rounded-full bg-[var(--popover-track)]" };
const FILL = { card: "rounded-sm", popover: "rounded-full" };

export function Meter({
  label,
  gauge,
  provider,
  className,
  describedBy,
  track = "card",
}: {
  label: string;
  gauge: Gauge;
  provider: AgentId;
  className: string;
  describedBy?: string;
  track?: keyof typeof TRACK;
}) {
  return (
    <div
      className={`overflow-hidden ${TRACK[track]} ${className}`}
      role="meter"
      aria-label={label}
      aria-describedby={describedBy}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={gauge.usedFigure ?? undefined}
      aria-valuetext={gauge.usedFigure === null ? "Usage unavailable" : undefined}
    >
      <div
        className={`h-full ${FILL[track]} transition-[width]`}
        style={{ width: `${gauge.usedNow ?? 0}%`, ...usageFillStyle(provider, gauge.meter) }}
      />
    </div>
  );
}

export function MeterRow({
  label,
  window,
  provider,
}: {
  label: string;
  window: LimitWindowPresentation;
  provider: AgentId;
}) {
  const noteId = useId();
  return (
    <div className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="text-[10px] leading-4 font-semibold tracking-[0.03em] text-[var(--mute)] uppercase">{label}</span>
        <span id={noteId} className="font-mono text-[11px] leading-snug text-[var(--mute)]">{window.note}</span>
      </div>
      <Meter label={label} gauge={window} provider={provider} className="h-1 w-24 shrink-0" describedBy={noteId} />
      <span className="w-11 shrink-0 text-right font-mono text-[12px]" style={{ color: usageTextColor(window.meter) }}>
        {window.text}
      </span>
    </div>
  );
}
