import { useMemo, useState } from "react";
import { copy } from "$lib/copy";
import { displayError, parseInvokeError } from "$lib/error";
import type { GithubRepo } from "$lib/installSource";
import {
  agentsAllowed,
  allKeys,
  installOutcomeClean,
  selectedItems,
  summarizeOutcomes,
  targetsFor,
  type MarketplaceAction,
} from "$lib/marketplaceSelection";
import type {
  AgentId,
  AgentInfo,
  InstallItemsRequest,
  InstallItemsResult,
  ItemPick,
  ItemScope,
  ProjectDto,
} from "$lib/types";
import { MarketplaceBrowser } from "./MarketplaceBrowser";
import { useMarketplaceInspect } from "./useMarketplaceInspect";

export type MarketplaceInstallProps = {
  repo: GithubRepo;
  agentName: string;
  hint: string;
  busy: boolean;
  itemsBusy: boolean;
  error: string | null;
  visibleAgents: AgentInfo[];
  currentAgentId?: AgentId;
  projects: ProjectDto[];
  currentScopePath: string | null;
  onCancel: () => void;
  onInstallPlugin: () => void;
  onInstallItems: (request: InstallItemsRequest) => Promise<InstallItemsResult | null>;
  onPickFolder: () => Promise<string | null>;
};

/**
 * The GitHub branch of the Install sheet: reads the marketplace, offers the three actions, and
 * owns the selection, targets, result, and footer. Lazy-loaded so the entry chunk stays small.
 */
export function MarketplaceInstall({
  repo,
  agentName,
  hint,
  busy,
  itemsBusy,
  error,
  visibleAgents,
  currentAgentId,
  projects,
  currentScopePath,
  onCancel,
  onInstallPlugin,
  onInstallItems,
  onPickFolder,
}: MarketplaceInstallProps) {
  const inspectQuery = useMarketplaceInspect(repo);
  const inspect = inspectQuery.data?.isMarketplace ? inspectQuery.data : null;

  const [action, setAction] = useState<MarketplaceAction>("plugin");
  const [keys, setKeys] = useState<Set<string>>(() => new Set());
  const [filter, setFilter] = useState("");
  const [providers, setProviders] = useState<AgentId[]>(() => (currentAgentId ? [currentAgentId] : []));
  const [scope, setScope] = useState<ItemScope>(() =>
    currentScopePath ? { kind: "project", projectPath: currentScopePath } : { kind: "global" },
  );
  const [result, setResult] = useState<InstallItemsResult | null>(null);
  const [itemsError, setItemsError] = useState<string | null>(null);

  const local = inspect !== null && action !== "plugin";
  const canAgents = agentsAllowed(providers);
  const picks: ItemPick[] = useMemo(() => {
    if (!inspect || !local) {
      return [];
    }
    const effective = action === "all" ? new Set(allKeys(inspect)) : keys;
    return selectedItems(inspect, effective).filter((pick) => pick.kind !== "agent" || canAgents);
  }, [action, canAgents, inspect, keys, local]);
  const anyBusy = busy || itemsBusy;
  const canSubmitItems = local && picks.length > 0 && providers.length > 0 && !anyBusy;

  async function submitItems(items: ItemPick[], overwriteUnmanaged: boolean) {
    if (!inspect || items.length === 0 || anyBusy) {
      return;
    }
    setItemsError(null);
    const request: InstallItemsRequest = {
      source: { owner: repo.owner, repo: repo.repo, ref: repo.ref ?? "HEAD" },
      commitSha: inspect.commitSha,
      items,
      targets: targetsFor(providers, scope),
      overwriteUnmanaged,
    };
    try {
      const next = await onInstallItems(request);
      if (next) {
        setResult(next);
      }
    } catch (caught) {
      setItemsError(displayError(parseInvokeError(caught), agentName));
    }
  }

  function overwriteConflicts() {
    if (!result) {
      return;
    }
    const wanted = new Set(summarizeOutcomes(result).conflicts.map((outcome) => `${outcome.kind}:${outcome.name}`));
    void submitItems(
      picks.filter((pick) => wanted.has(`${pick.kind}:${itemName(pick)}`)),
      true,
    );
  }

  const note = (() => {
    if (inspectQuery.isPending) {
      return { tone: "mute" as const, text: copy.marketplaceLoading };
    }
    if (inspectQuery.isError) {
      return {
        tone: "trip" as const,
        text: copy.marketplaceFailed(displayError(parseInvokeError(inspectQuery.error), agentName)),
      };
    }
    if (inspectQuery.data && !inspectQuery.data.isMarketplace) {
      return { tone: "mute" as const, text: copy.marketplaceNotFound };
    }
    return null;
  })();

  const submitLabel = local
    ? itemsBusy
      ? copy.installing
      : copy.installItemsButton(picks.length)
    : busy
      ? copy.installing
      : copy.install;
  const submitDisabled = local ? !canSubmitItems : anyBusy;
  const done = result !== null && installOutcomeClean(result);
  const shownError = itemsError ?? error;

  return (
    <>
      {!inspect ? <p className="text-[11.5px] text-[var(--mute)]">{hint}</p> : null}
      {note ? (
        <p
          className={`text-[11.5px] ${note.tone === "trip" ? "text-[var(--trip)]" : "text-[var(--mute)]"}`}
          role={note.tone === "trip" ? "alert" : undefined}
        >
          {note.text}
        </p>
      ) : null}
      {inspect ? (
        <MarketplaceBrowser
          inspect={inspect}
          action={action}
          onAction={(next) => {
            setAction(next);
            setResult(null);
          }}
          keys={keys}
          onKeysChange={setKeys}
          filter={filter}
          onFilterChange={setFilter}
          providers={providers}
          onProvidersChange={setProviders}
          scope={scope}
          onScopeChange={setScope}
          visibleAgents={visibleAgents}
          projects={projects}
          onPickFolder={onPickFolder}
          result={result}
          busy={anyBusy}
          onOverwriteConflicts={overwriteConflicts}
        />
      ) : null}
      {shownError ? (
        <p
          className="border-l-[3px] border-[var(--trip)] bg-[var(--well)] px-2.5 py-2 font-mono text-xs text-[var(--silkscreen)]"
          role="alert"
        >
          {shownError}
        </p>
      ) : null}
      <footer className="mt-1 flex justify-end gap-2">
        <button
          type="button"
          className="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12.5px] text-[var(--silkscreen)]"
          onClick={onCancel}
        >
          {done ? "Close" : copy.cancel}
        </button>
        <button
          type="button"
          className="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-4 text-[11.5px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] disabled:opacity-45"
          disabled={submitDisabled}
          onClick={() => {
            if (local) {
              void submitItems(picks, false);
            } else {
              onInstallPlugin();
            }
          }}
        >
          {submitLabel}
        </button>
      </footer>
    </>
  );
}

/** Folder or file stem the backend will use as the item name — mirrors `item_name` in Rust. */
function itemName(pick: ItemPick): string {
  const last = pick.path.replace(/\\/g, "/").replace(/\/+$/, "").split("/").pop() ?? "";
  return pick.kind === "agent" ? last.replace(/\.md$/i, "") : last;
}
