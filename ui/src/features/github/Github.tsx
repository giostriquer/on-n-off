import { RefreshCw } from "lucide-react";
import { displayError, parseInvokeError } from "$lib/error";
import { formatUpdatedAgo, listCountLabel, orderPrs, statusHeadline } from "$lib/githubFormat";
import type { GithubPrList, GithubPrs } from "$lib/githubTypes";
import type { GithubPollSeconds } from "$lib/types";
import { PrRow } from "./PrRow";
import { useGithubPrs } from "./useGithubPrs";

type GithubProps = {
  pollSeconds: GithubPollSeconds;
  /** Opens the Settings screen, where the scope and interval live. */
  onOpenSettings?: () => void;
};

const EMPTY_LIST: GithubPrList = { total: 0, items: [] };

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

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]" data-testid="github-screen" aria-busy={loading}>
      <header className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1 font-mono text-[12px] text-[var(--mute)]" data-testid="github-caption">
          Pull requests
          {prs?.viewer ? ` · ${prs.viewer}` : ""}
          {updated ? ` · updated ${updated}` : ""}
          {loading ? (
            <span className="ml-2" role="status" aria-live="polite">
              · Checking…
            </span>
          ) : null}
        </div>
        <ScopeChips scope={prs?.scope ?? []} onOpenSettings={onOpenSettings} />
        <button
          type="button"
          className="inline-flex size-7 items-center justify-center rounded-md border border-[var(--hair)] bg-transparent text-[var(--mute)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fill)] disabled:opacity-45"
          disabled={loading}
          aria-label="Refresh pull requests"
          onClick={refresh}
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
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

      <div className="flex flex-col gap-3">
        <PrList
          title="Review requested"
          list={prs?.reviewRequested ?? EMPTY_LIST}
          empty="No reviews requested from you."
          checking={checking}
          now={now}
        />
        <PrList
          title="Mine"
          list={prs?.mine ?? EMPTY_LIST}
          empty="No open pull requests of yours."
          checking={checking}
          now={now}
        />
        <PrList
          title="Assigned"
          list={prs?.assigned ?? EMPTY_LIST}
          empty="Nothing assigned to you."
          checking={checking}
          now={now}
        />
      </div>

      <p className="font-mono text-[10.5px] leading-snug text-[var(--mute)]">
        read-only · signed in through `gh auth login`, nothing is written to GitHub · refreshed every{" "}
        {pollSeconds}s while this window is visible · the CI dot opens the checks tab
      </p>
    </div>
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
          className="border-0 bg-transparent p-0 text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase hover:text-[var(--silkscreen)]"
          onClick={onOpenSettings}
        >
          Scope
        </button>
      ) : null}
    </div>
  );
}

function PrList({
  title,
  list,
  empty,
  checking,
  now,
}: {
  title: string;
  list: GithubPrList;
  empty: string;
  checking: boolean;
  now: number;
}) {
  return (
    <section
      className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
      aria-label={title}
    >
      <header className="border-b border-[var(--hair)] px-3.5 py-2.5">
        <h2 className="m-0 flex items-center gap-2 text-[11.5px] font-semibold tracking-[0.03em] uppercase">
          {title}
          <span className="font-mono text-[11px] font-normal text-[var(--mute)]">{listCountLabel(list)}</span>
        </h2>
      </header>
      {list.items.length ? (
        <ul className="m-0 list-none p-0" role="list">
          {orderPrs(list.items).map((pr) => (
            <PrRow key={pr.id} pr={pr} now={now} />
          ))}
        </ul>
      ) : (
        <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">{checking ? "Checking…" : empty}</p>
      )}
    </section>
  );
}
