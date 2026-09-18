import { TooltipButton } from "$lib/TooltipButton";
import { FOCUS_RING } from "$lib/a11y";
import { copy } from "$lib/copy";
import type { AgentTabDto, HookDto } from "$lib/types";

type HookListProps = {
  tab: AgentTabDto;
  /** Already sorted and filtered by the route; this component never derives the list again. */
  hooks: HookDto[];
  filterQuery?: string;
  /** Set for a provider whose hooks on-n-off does not read, which is not the same as none. */
  unread?: string;
};

/** "mcp_tool" is the provider's word; the badge is the one place it is made presentable. */
function handlerLabel(handler: string): string {
  return handler.replace(/_/g, " ").toUpperCase();
}

function Badge({ children }: { children: string }) {
  return (
    <span className="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[10px] font-semibold tracking-[0.03em] text-[var(--mute)]">
      {children}
    </span>
  );
}

/**
 * Every hook handler a provider would run, one row each, with where it comes from. Nothing here
 * is togglable: on-n-off reads hooks and never writes or runs them, so a Codex handler switched
 * off in `[hooks.state]` is shown as inactive rather than as something to switch back on.
 */
export function HookList({ tab, hooks, filterQuery = "", unread }: HookListProps) {
  const pool = tab.hooks ?? [];
  const live = pool.filter((hook) => hook.enabled).length;

  return (
    <div className="flex flex-col gap-3.5 px-5 pt-[18px] pb-[26px]">
      <header className="flex items-baseline gap-3">
        <h2 className="m-0 shrink-0 text-[15px] font-semibold tracking-[0.05em] uppercase whitespace-nowrap">Hooks</h2>
        {unread ? null : (
          <span className="font-mono text-xs leading-snug text-[var(--mute)]">
            {live} active · {copy.hooksScope}
          </span>
        )}
      </header>

      {unread ? (
        <p className="text-[13px] text-[var(--mute)]">{unread}</p>
      ) : hooks.length === 0 ? (
        <p className="text-[13px] text-[var(--mute)]">
          {filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyHooks}
        </p>
      ) : (
        <div className="flex flex-col gap-1.5">
          {hooks.map((hook) => (
            <article key={hook.id} className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
              <div className="flex items-start gap-3 px-3 py-[11px]">
                <span
                  className={`mt-2 size-2 shrink-0 rounded-full ${
                    hook.enabled ? "bg-[var(--live)] shadow-[0_0_7px_var(--live)]" : "bg-[var(--mute)]"
                  }`}
                  aria-hidden="true"
                />
                <div className="w-[238px] min-w-0 shrink-0">
                  <div className="flex flex-wrap items-baseline gap-2">
                    <span
                      className={`text-[13px]/[1.15] font-semibold break-words ${
                        hook.enabled ? "" : "text-[var(--mute)]"
                      }`}
                    >
                      {hook.event}
                    </span>
                    {hook.matcher ? (
                      <span className="font-mono text-[11px]/[1.3] break-all text-[var(--mute)]">{hook.matcher}</span>
                    ) : null}
                    {hook.enabled ? null : <Badge>OFF</Badge>}
                  </div>
                  <div className="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]" title={hook.source}>
                    {hook.source}
                  </div>
                  {hook.description ? (
                    <div className="mt-0.5 line-clamp-2 text-[11px]/[1.35] break-words text-[var(--mute)]">
                      {hook.description}
                    </div>
                  ) : null}
                </div>
                <div className="flex min-w-0 flex-1 flex-col items-start gap-1">
                  <Badge>{handlerLabel(hook.handler)}</Badge>
                  {hook.command ? (
                    <TooltipButton
                      label={`Full command for ${hook.event} from ${hook.source}`}
                      tooltip={<span className="font-mono text-[11px] break-all whitespace-pre-wrap">{hook.command}</span>}
                      className={`block w-full truncate rounded-none border-0 bg-transparent p-0 text-left font-mono text-[11px]/[1.5] text-[var(--mute)] ${FOCUS_RING}`}
                    >
                      {hook.command}
                    </TooltipButton>
                  ) : null}
                </div>
              </div>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}
