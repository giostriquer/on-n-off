import * as api from "$lib/api";
import {
  ciLabel,
  ciTone,
  ciToneColor,
  formatUpdatedAgo,
  reviewDecisionLabel,
  reviewDecisionTone,
  type CiTone,
} from "$lib/githubFormat";
import type { GithubPr } from "$lib/githubTypes";

function Badge({ children, tone = "mute" }: { children: string; tone?: CiTone }) {
  const color = ciToneColor(tone);
  return (
    <span
      className="shrink-0 rounded-md border px-1.5 py-px font-mono text-[10px] uppercase"
      style={{ borderColor: tone === "mute" ? "var(--hair)" : color, color }}
    >
      {children}
    </span>
  );
}

/**
 * One pull request: a CI dot that opens the checks tab, then the row itself (repo#number, title,
 * badges, author, branches, age) as one button whose accessible name is its content.
 */
export function PrRow({ pr, now }: { pr: GithubPr; now: number }) {
  const tone = ciTone(pr.ci);
  const color = ciToneColor(tone);
  const decision = reviewDecisionLabel(pr.reviewDecision);
  return (
    <li className="flex items-center gap-2.5 border-t border-[var(--hair)] px-3.5 py-2 first:border-t-0">
      <button
        type="button"
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-md border-0 bg-transparent p-0 hover:bg-[var(--well)]"
        aria-label={`${ciLabel(pr.ci)} · open checks`}
        title={ciLabel(pr.ci)}
        onClick={() => void api.openUrl(`${pr.url}/checks`)}
      >
        <span
          className="size-2 rounded-full"
          style={{ background: color, boxShadow: tone === "mute" ? undefined : `0 0 7px ${color}` }}
          aria-hidden="true"
        />
      </button>
      <button
        type="button"
        className="flex min-w-0 flex-1 flex-col gap-0.5 border-0 bg-transparent p-0 text-left hover:text-[var(--silkscreen)]"
        title="Open on GitHub"
        onClick={() => void api.openUrl(pr.url)}
      >
        <span className="flex min-w-0 items-center gap-2">
          <span className="shrink-0 font-mono text-[11px] text-[var(--mute)]">
            {pr.repo}#{pr.number}
          </span>
          <span className="min-w-0 truncate text-[13px] text-[var(--silkscreen)]">{pr.title}</span>
          {pr.isDraft ? <Badge>Draft</Badge> : null}
          {decision ? <Badge tone={reviewDecisionTone(pr.reviewDecision)}>{decision}</Badge> : null}
          {pr.reviewRequest === "team" ? <Badge>team</Badge> : null}
        </span>
        <span className="flex min-w-0 items-center gap-2 font-mono text-[10.5px] text-[var(--mute)]">
          <span className="shrink-0">{pr.author}</span>
          <span className="min-w-0 truncate">{`${pr.headRef} → ${pr.baseRef}`}</span>
        </span>
      </button>
      <span className="shrink-0 font-mono text-[11px] text-[var(--mute)]">{formatUpdatedAgo(pr.updatedAt, now)}</span>
    </li>
  );
}
