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

/** One circular glyph per state with its own distinct mark, so CI is readable without colour. */
const CI_GLYPH: Record<CiState, LucideIcon> = {
  success: CircleCheck,
  failure: CircleX,
  error: CircleAlert,
  pending: CircleDashed,
  none: Circle,
};

/**
 * A tight box — no inherited line-height, one more pixel above than below — so the mono capitals
 * sit on the title's optical centre instead of riding high in a box taller than the title itself.
 */
function Badge({ children, tone = "mute" }: { children: string; tone?: CiTone }) {
  const color = ciToneColor(tone);
  return (
    <span
      className="shrink-0 rounded-md border px-1.5 pt-[3px] pb-[2px] font-mono text-[10px] leading-none uppercase"
      style={{ borderColor: tone === "mute" ? "var(--hair)" : color, color }}
    >
      {children}
    </span>
  );
}

/**
 * One pull request: a CI glyph that opens the checks tab, then the row itself as one button whose
 * accessible name is its content — `#number`, title and badges (draft, review decision, merge
 * state, team) first, author and branches on the second line (the repository is named by the
 * list or group around the row) — and the age with its absolute time in a tooltip.
 */
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
          <span className="shrink-0 font-mono text-[11.5px] text-[var(--mute)]">#{pr.number}</span>
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
        <span className="flex min-w-0 items-center gap-1.5 font-mono text-[10.5px] text-[var(--mute)]">
          <span className="shrink-0">{pr.author}</span>
          <span aria-hidden="true">·</span>
          <span className="min-w-0 truncate">{`${pr.headRef} → ${pr.baseRef}`}</span>
        </span>
      </button>
      <time
        dateTime={pr.updatedAt}
        title={formatObservedAt(pr.updatedAt)}
        className="shrink-0 font-mono text-[11px] text-[var(--mute)]"
      >
        {formatAgo(pr.updatedAt, now)}
      </time>
    </li>
  );
}
