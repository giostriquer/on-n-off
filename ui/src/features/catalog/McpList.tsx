import { useState } from "react";
import { Rocker } from "@/features/agents/Rocker";
import { copy } from "$lib/copy";
import { isProjectOrigin } from "$lib/project";
import type { AgentTabDto, McpServerDto } from "$lib/types";

type Chip = "all" | "on" | "off";

type McpListProps = {
  tab: AgentTabDto;
  servers: McpServerDto[];
  filterQuery?: string;
  busy?: boolean;
  /** Shown under the header when this provider's servers can only be listed, not switched. */
  notice?: string;
  onToggle: (server: McpServerDto, enabled: boolean) => void;
};

export function McpList({ tab, servers: pool, filterQuery = "", busy = false, notice, onToggle }: McpListProps) {
  const [chip, setChip] = useState<Chip>("all");
  const servers =
    chip === "on" ? pool.filter((server) => server.enabled) : chip === "off" ? pool.filter((server) => !server.enabled) : pool;
  const live = tab.mcpServers.filter((server) => server.enabled).length;
  const hasProject = tab.mcpServers.some((server) => isProjectOrigin(server.origin));

  return (
    <div className="flex flex-col gap-3.5 px-5 pt-[18px] pb-[26px]">
      <header className="flex items-baseline gap-3">
        <h2 className="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">MCP servers</h2>
        <span className="font-mono text-xs leading-snug text-[var(--mute)]">
          {live} live · {hasProject ? "global + this project" : "user-scope config only"} · handshake not probed
        </span>
        <div className="flex-1" />
        <div className="flex border border-[var(--hair)]" role="group" aria-label="Filter list">
          {(["all", "on", "off"] as Chip[]).map((next) => (
            <button
              key={next}
              type="button"
              className={`h-[26px] rounded-none border-0 px-3 text-[10.5px] font-semibold tracking-[0.03em] uppercase ${
                chip === next ? "bg-[var(--well)] text-[var(--silkscreen)]" : "bg-transparent text-[var(--mute)]"
              }`}
              aria-pressed={chip === next}
              onClick={() => setChip(next)}
            >
              {next}
            </button>
          ))}
        </div>
      </header>

      {notice ? (
        <p
          role="note"
          className="rounded-[9px] border border-[var(--hair)] bg-[var(--well)] px-3 py-2 text-[12.5px]/[1.45] text-[var(--mute)]"
        >
          {notice}
        </p>
      ) : null}

      {pool.length === 0 ? (
        <p className="text-[13px] text-[var(--mute)]">
          {filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyMcps}
        </p>
      ) : servers.length === 0 ? (
        <p className="text-[13px] text-[var(--mute)]">
          {chip === "on" ? "No MCP servers on." : "No MCP servers off."}
        </p>
      ) : (
        <div className="flex flex-col gap-1.5">
          {servers.map((server) => (
            <article key={server.id} className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
              <div className="flex items-center gap-3 px-3 py-[11px]">
                <span
                  className="flex min-h-6 min-w-6 shrink-0 items-center justify-center text-[var(--mute)]"
                  aria-hidden="true"
                >
                  ·
                </span>
                <span
                  className={`size-2 shrink-0 rounded-full ${
                    server.enabled ? "bg-[var(--live)] shadow-[0_0_7px_var(--live)]" : "bg-[var(--mute)]"
                  }`}
                  aria-hidden="true"
                />
                <div className="w-[238px] min-w-0 shrink-0">
                  <div className="flex items-baseline gap-2">
                    <span className="text-[16px]/[1.15] font-semibold break-words">{server.name}</span>
                    <span className="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--mute)]">
                      {server.system.toUpperCase()}
                    </span>
                    {isProjectOrigin(server.origin) ? (
                      <span className="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--mute)]">
                        PROJECT
                      </span>
                    ) : null}
                  </div>
                  <div
                    className="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]"
                    title={server.source}
                  >
                    {server.source}
                  </div>
                </div>
                <div className="ml-auto shrink-0">
                  <Rocker
                    size="plugin"
                    on={server.enabled}
                    busy={busy}
                    disabled={!server.togglable}
                    ariaLabel={`${server.name} ${server.enabled ? "on" : "off"}`}
                    onToggle={() => onToggle(server, !server.enabled)}
                  />
                </div>
              </div>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}
