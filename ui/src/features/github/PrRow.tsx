import { Circle, CircleAlert, CircleCheck, CircleDashed, CircleX, type LucideIcon } from "lucide-react";
import { FOCUS_RING } from "$lib/a11y";
import * as api from "$lib/api";
import {
  ciLabel,
  ciTone,
  ciToneColor,
  mergeBadge,
  reviewBadge,
  type CiTone,
} from "$lib/githubFormat";
import type { CiState, GithubPr } from "$lib/githubTypes";
import { formatObservedAt } from "$lib/limitsFormat";
import { formatAgo } from "$lib/timeFormat";

const CI_GLYPH: Record<CiState, LucideIcon> = {
  success: CircleCheck,
  failure: CircleX,
  error: CircleAlert,
  pending: CircleDashed,
  none: Circle,
};

function Badge({ children, tone = "mute" }: { children: string; tone?: CiTone }) {
  const color = ciToneColor(tone);
  return (
    <span
      className="shrink-0 rounded-md border px-1.5 py-0.5 type-badge uppercase"
      style={{ borderColor: tone === "mute" ? "var(--hair)" : color, color }}
    >
      {children}
    </span>
  );
}

export function PrRow({ pr, now }: { pr: GithubPr; now: number }) {
  const tone = ciTone(pr.ci);
  const color = ciToneColor(tone);
  const Glyph = CI_GLYPH[pr.ci];
  const badges = [reviewBadge(pr.reviewDecision), mergeBadge(pr)];
  return (
    <li className="flex items-center gap-3 border-t border-[var(--hair)] px-3.5 py-2 first:border-t-0">
      <button
        type="button"
        className={`inline-flex size-6 shrink-0 items-center justify-center rounded-full border-0 bg-transparent p-0 hover:bg-[var(--well)] ${FOCUS_RING}`}
        style={{ color }}
        aria-label={`${ciLabel(pr.ci)} · open checks`}
        title={`${ciLabel(pr.ci)} · open checks`}
        onClick={() => void api.openUrl(`${pr.url}/checks`)}
      >
        <Glyph className="size-5" strokeWidth={2} aria-hidden="true" />
      </button>
      <button
        type="button"
        className={`flex min-w-0 flex-1 flex-col gap-0.5 rounded-md border-0 bg-transparent p-0 text-left hover:text-[var(--silkscreen)] ${FOCUS_RING}`}
        title="Open on GitHub"
        onClick={() => void api.openUrl(pr.url)}
      >
        <span className="flex min-w-0 items-center gap-2">
          <span className="shrink-0 font-mono text-[11px] text-[var(--mute)]">#{pr.number}</span>
          <span className="min-w-0 truncate text-[13px] text-[var(--silkscreen)]">{pr.title}</span>
          {pr.isDraft ? <Badge>Draft</Badge> : null}
          {badges.map((badge) =>
            badge ? (
              <Badge key={badge.label} tone={badge.tone}>
                {badge.label}
              </Badge>
            ) : null,
          )}
          {pr.reviewRequest === "team" ? <Badge>team</Badge> : null}
        </span>
        <span className="flex min-w-0 items-center gap-1.5 font-mono text-[10px] text-[var(--mute)]">
          <span className="shrink-0">{pr.author}</span>
          <span aria-hidden="true">·</span>
          <span className="min-w-0 truncate">{`${pr.headRef} → ${pr.baseRef}`}</span>
        </span>
      </button>
      <time
        dateTime={pr.updatedAt}
        title={formatObservedAt(pr.updatedAt)}
        className="shrink-0 font-mono text-[10px] text-[var(--mute)]"
      >
        {formatAgo(pr.updatedAt, now)}
      </time>
    </li>
  );
}
