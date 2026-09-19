import { Clock3 } from "lucide-react";
import { TooltipButton } from "$lib/TooltipButton";

export function UsageStatusBadge({ detail }: { detail: string }) {
  return <TooltipButton label="Usage status: Last known usage" tooltip={<>
    <div>Showing the last recorded usage.</div>
    <div className="mt-1 text-[var(--mute)]">{detail}</div>
  </>} className="inline-flex size-5 shrink-0 items-center justify-center rounded border-0 bg-transparent text-[var(--trip)] hover:opacity-80">
    <Clock3 className="size-3.5" aria-hidden="true" />
  </TooltipButton>;
}
