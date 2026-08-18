import { useCallback, useState } from "react";
import { RefreshCw } from "lucide-react";
import { ConfirmDialog } from "@/features/catalog/ConfirmDialog";
import { ItemList, ManagedItemStrip } from "@/features/catalog/ItemList";
import { useItemActions, useItemStatus } from "@/features/catalog/useItemStatus";
import { useAgentSession } from "@/features/session/SessionProvider";
import { copy } from "$lib/copy";
import { displayError, parseInvokeError } from "$lib/error";
import { statusForSkill, upstreamLabel } from "$lib/itemStatus";
import type { ItemStatus, SkillDto } from "$lib/types";

type Pending = { kind: "update"; status: ItemStatus } | { kind: "remove"; status: ItemStatus };

export function SkillsRoute() {
  const session = useAgentSession();
  const provider = session.currentAgent.id;
  const { sets, fetching, refresh } = useItemStatus(provider, session.currentScopePath);
  const { loadTab } = session;
  const reload = useCallback(() => loadTab(provider), [loadTab, provider]);
  const actions = useItemActions(provider, reload);
  const [pending, setPending] = useState<Pending | null>(null);
  const [acting, setActing] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const statusFor = useCallback((skill: SkillDto) => statusForSkill(skill, sets), [sets]);
  const agents = [...sets.global, ...sets.project].filter((status) => status.kind === "agent");

  async function run(task: () => Promise<void>) {
    setActing(true);
    setActionError(null);
    try {
      await task();
      setPending(null);
    } catch (error) {
      setActionError(displayError(parseInvokeError(error), session.currentAgent.displayName));
    } finally {
      setActing(false);
    }
  }

  return (
    <>
      <ItemList
        kind="skill"
        tab={session.currentTab.dto ?? session.emptyTabDto()}
        items={session.filtered?.skills ?? []}
        filterQuery={session.currentTab.filter}
        expandedIds={session.expandedIds}
        cliOk={session.currentAgent.cliOk}
        pluginToggle={session.currentAgent.pluginToggle}
        busy={session.currentTab.inFlight}
        onToggleExpand={session.toggleExpand}
        onTogglePlugin={session.togglePlugin}
        onToggleSkill={session.toggleSkill}
        onUninstall={(plugin) => session.setUninstallTarget(plugin)}
        statusFor={statusFor}
        onUpdateItem={(status) => setPending({ kind: "update", status })}
        onRemoveItem={(status) => setPending({ kind: "remove", status })}
        headerActions={
          <button
            type="button"
            className="mr-2 flex h-[26px] items-center gap-1.5 border border-[var(--hair)] bg-transparent px-2.5 text-[10.5px] font-semibold tracking-[0.03em] text-[var(--mute)] uppercase disabled:opacity-45"
            disabled={fetching}
            onClick={() => void refresh()}
          >
            <RefreshCw className={`size-3 ${fetching ? "animate-spin" : ""}`} aria-hidden="true" />
            {copy.checkUpdates}
          </button>
        }
      />
      {provider === "claude" && agents.length > 0 ? (
        <section className="flex flex-col gap-2 px-5 pb-[26px]" aria-label={copy.agentsSection}>
          <h3 className="m-0 text-[13px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            {copy.agentsSection}
          </h3>
          <div className="flex flex-col gap-1.5">
            {agents.map((status) => (
              <article key={status.id} className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
                <div className="flex items-start gap-3 px-3 py-[11px]">
                  <span className="mt-2 size-2 shrink-0 rounded-full bg-[var(--live)]" aria-hidden="true" />
                  <div className="min-w-0 flex-1">
                    <div className="text-[16px]/[1.15] font-semibold break-words">{status.displayName}</div>
                    <div className="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]" title={status.targetPath}>
                      {status.targetPath}
                    </div>
                  </div>
                </div>
                <ManagedItemStrip
                  status={status}
                  busy={acting}
                  onUpdateItem={(next) => setPending({ kind: "update", status: next })}
                  onRemoveItem={(next) => setPending({ kind: "remove", status: next })}
                />
              </article>
            ))}
          </div>
        </section>
      ) : null}
      {actionError ? (
        <p className="mx-5 border-l-[3px] border-[var(--trip)] bg-[var(--well)] px-2.5 py-2 font-mono text-xs" role="alert">
          {actionError}
        </p>
      ) : null}
      {pending?.kind === "update" ? (
        <ConfirmDialog
          title={copy.updateItemTitle(pending.status.displayName)}
          body={
            pending.status.modified
              ? copy.updateItemBodyModified(upstreamLabel(pending.status))
              : copy.updateItemBody(upstreamLabel(pending.status))
          }
          confirmLabel={copy.overwriteBackup}
          alternate={{
            label: copy.keepMine,
            onClick: () => void run(() => actions.update(pending.status, "dismiss")),
          }}
          busy={acting}
          onCancel={() => setPending(null)}
          onConfirm={() => void run(() => actions.update(pending.status, "overwrite"))}
        />
      ) : null}
      {pending?.kind === "remove" ? (
        <ConfirmDialog
          title={copy.removeItemTitle(pending.status.displayName)}
          body={copy.removeItemBody}
          confirmLabel={copy.removeItem}
          busy={acting}
          onCancel={() => setPending(null)}
          onConfirm={() => void run(() => actions.remove(pending.status))}
        />
      ) : null}
    </>
  );
}
