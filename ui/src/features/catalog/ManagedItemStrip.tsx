import { copy } from "$lib/copy";
import { itemBadges } from "$lib/itemStatus";
import type { ItemStatus } from "$lib/types";

const BADGE_TONE = {
  mute: "border-[var(--hair)] text-[var(--mute)]",
  warn: "border-[var(--warn)] text-[var(--warn)]",
  trip: "border-[var(--trip)] text-[var(--trip)]",
} as const;

export type ManagedItemActions = {
  onUpdateItem: (status: ItemStatus) => void;
  onRemoveItem: (status: ItemStatus) => void;
};

/** Version / update / modified badges plus Update and Remove for an item on-n-off installed. */
export function ManagedItemStrip({
  status,
  busy,
  onUpdateItem,
  onRemoveItem,
}: { status: ItemStatus; busy: boolean } & ManagedItemActions) {
  const updatable = status.upstream.state === "updateAvailable" && !status.missing;
  return (
    <div className="flex flex-wrap items-center gap-1.5 border-t border-[var(--hair)] px-3 py-[7px] pl-[35px]">
      {itemBadges(status).map((badge) => (
        <span
          key={badge.label}
          className={`border px-1.5 py-0.5 font-mono text-[10px] tracking-[0.02em] ${BADGE_TONE[badge.tone]}`}
        >
          {badge.label}
        </span>
      ))}
      <span className="flex-1" />
      {updatable ? (
        <button
          type="button"
          className="h-6 border border-[var(--warn)] bg-transparent px-2.5 text-[10px] font-semibold tracking-[0.03em] text-[var(--warn)] uppercase disabled:opacity-45"
          aria-label={`Update ${status.displayName}`}
          disabled={busy}
          onClick={() => onUpdateItem(status)}
        >
          {copy.update}
        </button>
      ) : null}
      <button
        type="button"
        className="h-6 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] px-2.5 text-[10px] font-semibold tracking-[0.03em] text-[var(--trip)] uppercase disabled:opacity-45"
        aria-label={`Remove ${status.displayName}`}
        disabled={busy}
        onClick={() => onRemoveItem(status)}
      >
        {copy.removeItem}
      </button>
    </div>
  );
}

/** A managed Claude subagent (`agents/*.md`), listed under the Skills library. */
export function AgentCard({
  status,
  busy,
  onUpdateItem,
  onRemoveItem,
}: { status: ItemStatus; busy: boolean } & ManagedItemActions) {
  return (
    <article className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
      <div className="flex items-start gap-3 px-3 py-[11px]">
        <span className="mt-2 size-2 shrink-0 rounded-full bg-[var(--live)]" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <div className="text-[16px]/[1.15] font-semibold break-words">{status.displayName}</div>
          <div className="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]" title={status.targetPath}>
            {status.targetPath}
          </div>
        </div>
      </div>
      <ManagedItemStrip status={status} busy={busy} onUpdateItem={onUpdateItem} onRemoveItem={onRemoveItem} />
    </article>
  );
}
