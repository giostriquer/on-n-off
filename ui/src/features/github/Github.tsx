import { useState } from "react";
import { ChevronDown, RefreshCw, Search, X } from "lucide-react";
import { FOCUS_RING } from "$lib/a11y";
import { displayError, parseInvokeError } from "$lib/error";
import {
  filterPrs,
  formatUpdatedAgo,
  listCountLabel,
  orderPrs,
  prsSummary,
  statusHeadline,
} from "$lib/githubFormat";
import type { GithubListId, GithubPrList, GithubPrs } from "$lib/githubTypes";
import type { GithubPollSeconds } from "$lib/types";
import { readCollapsed, writeCollapsed } from "./collapsed";
import { PrRow } from "./PrRow";
import { useGithubPrs } from "./useGithubPrs";

type GithubProps = {
  pollSeconds: GithubPollSeconds;
  /** Opens the Settings screen, where the scope and interval live. */
  onOpenSettings?: () => void;
};

const EMPTY_LIST: GithubPrList = { total: 0, items: [] };

/** Mine first: the user's own CI is what the screen exists for, and review requests can be many. */
const SECTIONS: { id: GithubListId; title: string; empty: string }[] = [
  { id: "mine", title: "Mine", empty: "No open pull requests of yours." },
  { id: "reviewRequested", title: "Review requested", empty: "No reviews requested from you." },
  { id: "assigned", title: "Assigned", empty: "Nothing assigned to you." },
];

type Banner = { headline: string; hint: string; tone: "trip" | "warn" };

/** What to tell the user when the read did not come back `ok`, or the command itself failed. */
function bannerFor(prs: GithubPrs | null, invokeError: unknown): Banner | null {
  if (invokeError) {
    return {
      headline: "GitHub unavailable",
      hint: displayError(parseInvokeError(invokeError), "GitHub"),
      tone: "trip",
    };
  }
  if (!prs || prs.status === "ok") return null;
  return {
    headline: statusHeadline(prs.status),
    hint: prs.hint ?? "",
    tone: prs.status === "rateLimited" || prs.status === "network" ? "warn" : "trip",
  };
}

export function Github({ pollSeconds, onOpenSettings }: GithubProps) {
  const { query, loading, now, refresh } = useGithubPrs(pollSeconds);
  const prs = query.data ?? null;
  const banner = bannerFor(prs, query.error);
  const updated = formatUpdatedAgo(prs?.fetchedAt, now);
  const checking = loading && !prs;
  const [search, setSearch] = useState("");
  const [collapsed, setCollapsed] = useState<Set<GithubListId>>(readCollapsed);

  function toggle(id: GithubListId) {
    const next = new Set(collapsed);
    if (!next.delete(id)) next.add(id);
    setCollapsed(next);
    writeCollapsed(next);
  }

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]" data-testid="github-screen" aria-busy={loading}>
      <header className="flex flex-wrap items-end gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">Pull requests</h2>
          <p className="mt-1 mb-0 font-mono text-[12px] text-[var(--mute)]" data-testid="github-caption">
            {prs?.viewer ? `${prs.viewer} · ` : ""}
            {updated ? `updated ${updated} · ` : ""}
            every {pollSeconds} s
            {checking ? (
              // Only the first load is announced; routine polls stay quiet for screen readers.
              <span role="status" aria-live="polite">
                {" "}
                · Checking…
              </span>
            ) : loading ? (
              <span aria-hidden="true"> · refreshing</span>
            ) : null}
          </p>
          {prs ? <Summary prs={prs} /> : null}
        </div>
        <ScopeChips scope={prs?.scope ?? []} onOpenSettings={onOpenSettings} />
        <button
          type="button"
          className={`inline-flex size-7 items-center justify-center rounded-md border border-[var(--hair)] bg-transparent text-[var(--mute)] disabled:opacity-45 ${FOCUS_RING}`}
          disabled={loading}
          aria-label="Refresh pull requests"
          onClick={refresh}
        >
          <RefreshCw className={`size-3.5 ${loading ? "motion-safe:animate-spin" : ""}`} aria-hidden="true" />
        </button>
      </header>

      {banner ? (
        <p
          role="alert"
          className={`m-0 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] px-3.5 py-2.5 text-[13px] ${
            banner.tone === "trip" ? "text-[var(--trip)]" : "text-[var(--warn)]"
          }`}
        >
          <span className="font-semibold">{banner.headline}</span>
          {banner.hint ? ` — ${banner.hint}` : ""}
        </p>
      ) : null}

      {prs?.stale ? (
        <p className="m-0 rounded-[11px] bg-[var(--well)] px-3.5 py-[7px] font-mono text-[11px] text-[var(--warn)]">
          Showing the last read from {updated || "earlier"} — GitHub could not be refreshed.
        </p>
      ) : null}

      <SearchField query={search} onChange={setSearch} />

      <div className="flex flex-col gap-3">
        {SECTIONS.map(({ id, title, empty }) => (
          <PrList
            key={id}
            id={id}
            title={title}
            list={prs?.[id] ?? EMPTY_LIST}
            query={search}
            empty={empty}
            checking={checking}
            collapsed={collapsed.has(id)}
            onToggle={() => toggle(id)}
            now={now}
          />
        ))}
      </div>

      <p className="font-mono text-[10.5px] leading-snug text-[var(--mute)]">
        read-only · signed in through `gh auth login`, nothing is written to GitHub · refreshed while this
        window is visible · the CI glyph opens the checks tab, the row opens the pull request
      </p>
    </div>
  );
}

/**
 * "1 mine · 1 failing · 15 to review · 0 assigned". Failing appears only when there is any, with
 * a "+" when only part of the user's pull requests were loaded.
 */
function Summary({ prs }: { prs: GithubPrs }) {
  const { mine, failing, failingIsPartial, review, assigned } = prsSummary(prs);
  return (
    <p className="mt-1.5 mb-0 text-[12.5px] text-[var(--silkscreen)]" data-testid="github-summary">
      {mine} mine
      {failing ? (
        <>
          {" · "}
          <span className="font-semibold text-[var(--trip)]">
            {failing}
            {failingIsPartial ? "+" : ""} failing
          </span>
        </>
      ) : null}
      {" · "}
      {review} to review
      {" · "}
      {assigned} assigned
    </p>
  );
}

function ScopeChips({ scope, onOpenSettings }: { scope: string[]; onOpenSettings?: () => void }) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5" aria-label="Scope">
      {scope.length ? (
        scope.map((qualifier) => (
          <span
            key={qualifier}
            className="rounded-md border border-[var(--hair)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--mute)]"
          >
            {qualifier}
          </span>
        ))
      ) : (
        <span className="font-mono text-[10px] text-[var(--mute)]">all repositories</span>
      )}
      {onOpenSettings ? (
        <button
          type="button"
          className={`rounded-sm border-0 bg-transparent px-1 py-0.5 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase hover:text-[var(--silkscreen)] ${FOCUS_RING}`}
          onClick={onOpenSettings}
        >
          Scope
        </button>
      ) : null}
    </div>
  );
}

/** One field narrows all three lists; Escape clears it. */
function SearchField({ query, onChange }: { query: string; onChange: (query: string) => void }) {
  return (
    <div className="relative">
      <Search
        className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-[var(--mute)]"
        aria-hidden="true"
      />
      <input
        type="search"
        className={`h-8 w-full rounded-md border border-[var(--hair)] bg-[var(--well)] pr-8 pl-8 font-mono text-[11.5px] text-[var(--silkscreen)] placeholder:text-[var(--mute)] [&::-webkit-search-cancel-button]:hidden ${FOCUS_RING}`}
        aria-label="Search pull requests"
        placeholder="title, #number, repository, author, branch"
        value={query}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") onChange("");
        }}
      />
      {query ? (
        <button
          type="button"
          className={`absolute top-1/2 right-1 inline-flex size-6 -translate-y-1/2 items-center justify-center rounded-sm border-0 bg-transparent p-0 text-[var(--mute)] hover:text-[var(--silkscreen)] ${FOCUS_RING}`}
          aria-label="Clear search"
          onClick={() => onChange("")}
        >
          <X className="size-3.5" aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}

function PrList({
  id,
  title,
  list,
  query,
  empty,
  checking,
  collapsed,
  onToggle,
  now,
}: {
  id: GithubListId;
  title: string;
  list: GithubPrList;
  query: string;
  empty: string;
  checking: boolean;
  collapsed: boolean;
  onToggle: () => void;
  now: number;
}) {
  const searching = query.trim().length > 0;
  const shown = searching ? filterPrs(list.items, query) : list.items;
  const count = listCountLabel(list, searching ? shown.length : undefined);
  // A search looks everywhere: a folded section with matches opens for as long as the search
  // lasts; one without stays folded, its header already saying "0 of N".
  const open = (searching && shown.length > 0) || !collapsed;
  const bodyId = `github-${id}-list`;
  return (
    <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label={title}>
      {/* Sticks to the top of the scroll area so a long list keeps its name in view. */}
      <header
        className={`sticky top-0 z-10 rounded-t-[11px] bg-[var(--plate)] ${open ? "border-b border-[var(--hair)]" : "rounded-b-[11px]"}`}
      >
        <h3 className="m-0">
          <button
            type="button"
            className={`flex h-10 w-full items-center gap-2 rounded-[11px] border-0 bg-transparent px-3.5 text-left text-[11.5px] font-semibold tracking-[0.03em] uppercase hover:bg-[var(--well)] ${FOCUS_RING}`}
            aria-expanded={open}
            aria-controls={open ? bodyId : undefined}
            onClick={onToggle}
          >
            <ChevronDown
              className={`size-3.5 shrink-0 text-[var(--mute)] transition-transform motion-reduce:transition-none ${open ? "" : "-rotate-90"}`}
              aria-hidden="true"
            />
            {title}
            <span className="font-mono text-[11px] font-normal normal-case text-[var(--mute)]">{count}</span>
          </button>
        </h3>
      </header>
      {!open ? null : shown.length ? (
        <ul id={bodyId} className="m-0 list-none p-0" role="list">
          {orderPrs(shown).map((pr) => (
            <PrRow key={pr.id} pr={pr} now={now} />
          ))}
        </ul>
      ) : (
        <p id={bodyId} className="px-3.5 py-4 text-[13px] text-[var(--mute)]">
          {checking ? "Checking…" : searching ? "No matches." : empty}
        </p>
      )}
    </section>
  );
}
