import type { ReactNode } from "react";
import { TooltipButton } from "$lib/TooltipButton";
import "./TermBadge.css";

export type TermTone = "neutral" | "warning" | "expired";

export function TermBadge({ badge: { name, label, tone, progress }, tooltip }: {
  badge: { name: string; label: string; tone: TermTone; progress: number };
  tooltip: ReactNode;
}) {
  return <TooltipButton label={name} tooltip={tooltip} className={`type-badge term-badge term-badge--${tone}`}>
    {tone === "warning" && <span aria-hidden="true" className="term-badge-fill" style={{width: `${progress * 100}%`, backgroundColor: `color-mix(in srgb, var(--expiry-fill-start), var(--expiry-fill-end) ${progress * 100}%)`}} />}
    <span className="relative">{label}</span>
  </TooltipButton>;
}
