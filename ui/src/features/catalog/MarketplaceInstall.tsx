import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { copy } from "$lib/copy";
import { displayError, parseInvokeError } from "$lib/error";
import type { GithubRepo } from "$lib/installSource";
import { summarizeOutcomes } from "$lib/itemOutcomes";
import { ITEM_STATUS_KEY } from "$lib/itemStatus";
import { agentsAllowed, allKeys, entryKey, selectedItems, targetsFor } from "$lib/marketplaceSelection";
import type {
  AgentId,
  AgentInfo,
  InstallItemsRequest,
  InstallItemsResult,
  ItemPick,
  ProjectDto,
} from "$lib/types";
import { MarketplaceBrowser, type MarketplaceSelection } from "./MarketplaceBrowser";
import { SheetError, SheetFooter } from "./SheetChrome";
import { useMarketplaceInspect } from "./useMarketplaceInspect";

export type MarketplaceInstallProps = {
  repo: GithubRepo;
  agentName: string;
  hint: string;
  busy: boolean;
  error: string | null;
  visibleAgents: AgentInfo[];
  currentAgentId: AgentId;
  projects: ProjectDto[];
  currentScopePath: string | null;
  onCancel: () => void;
  onInstallPlugin: () => void;
  onInstallItems: (request: InstallItemsRequest) => Promise<InstallItemsResult | null>;
  onPickFolder: () => Promise<string | null>;
};

/**
 * The GitHub branch of the Install sheet: reads the marketplace, offers the three actions, and
 * owns the selection, targets, result, and footer.
 */
export function MarketplaceInstall({
  repo,
  agentName,
  hint,
  busy,
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
  const queryClient = useQueryClient();

  const [selection, setSelection] = useState<MarketplaceSelection>(() => ({
    action: "plugin",
    keys: new Set(),
    filter: "",
    providers: [currentAgentId],
    scope: currentScopePath ? { kind: "project", projectPath: currentScopePath } : { kind: "global" },
  }));
  const [result, setResult] = useState<InstallItemsResult | null>(null);
  const [itemsError, setItemsError] = useState<string | null>(null);

  const local = inspect !== null && selection.action !== "plugin";
  const picks: ItemPick[] = useMemo(() => {
    if (!inspect || !local) {
      return [];
    }
    const effective = selection.action === "all" ? new Set(allKeys(inspect)) : selection.keys;
    const canAgents = agentsAllowed(selection.providers);
    return selectedItems(inspect, effective).filter((pick) => pick.kind !== "agent" || canAgents);
  }, [inspect, local, selection.action, selection.keys, selection.providers]);
  const canSubmitItems = local && picks.length > 0 && selection.providers.length > 0 && !busy;

  function patchSelection(patch: Partial<MarketplaceSelection>) {
    setSelection((prev) => ({ ...prev, ...patch }));
    if (patch.action !== undefined) {
      setResult(null);
    }
  }

  async function submitItems(items: ItemPick[], overwriteUnmanaged: boolean) {
    if (!inspect || items.length === 0 || busy) {
      return;
    }
    setItemsError(null);
    const request: InstallItemsRequest = {
      source: { owner: repo.owner, repo: repo.repo, ref: repo.ref ?? "HEAD" },
      commitSha: inspect.commitSha,
      items,
      targets: targetsFor(selection.providers, selection.scope),
      overwriteUnmanaged,
    };
    try {
      const next = await onInstallItems(request);
      if (next) {
        setResult(next);
        // Newly installed items must show their badges at once, not after the status staleTime.
        for (const provider of summarizeOutcomes(next).touchedProviders) {
          void queryClient.invalidateQueries({ queryKey: [ITEM_STATUS_KEY, provider] });
        }
      }
    } catch (caught) {
      setItemsError(displayError(parseInvokeError(caught), agentName));
    }
  }

  function overwriteConflicts() {
    if (!result) {
      return;
    }
    const wanted = new Set(
      summarizeOutcomes(result).conflicts.map((outcome) => entryKey(outcome.pluginName, outcome.kind, outcome.path)),
    );
    void submitItems(
      picks.filter((pick) => wanted.has(entryKey(pick.pluginName, pick.kind, pick.path))),
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

  const submitLabel = busy ? copy.installing : local ? copy.installItemsButton(picks.length) : copy.install;

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
          selection={selection}
          onChange={patchSelection}
          visibleAgents={visibleAgents}
          projects={projects}
          onPickFolder={onPickFolder}
          result={result}
          busy={busy}
          onOverwriteConflicts={overwriteConflicts}
        />
      ) : null}
      <SheetError message={itemsError ?? error} />
      <SheetFooter
        submitLabel={submitLabel}
        submitDisabled={local ? !canSubmitItems : busy}
        onCancel={onCancel}
        onSubmit={() => {
          if (local) {
            void submitItems(picks, false);
          } else {
            onInstallPlugin();
          }
        }}
      />
    </>
  );
}
