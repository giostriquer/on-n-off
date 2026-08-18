import { useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { FolderPlus, Search } from "lucide-react";
import { ProviderIcon } from "$lib/ProviderIcon";
import { agentRoot, mcpConfigPath } from "$lib/catalog";
import { sameProjectPath } from "$lib/project";
import { tripTagClass, type TripEntry } from "$lib/tripLog";
import type { AgentId, AgentInfo, AgentTabDto, ProjectDto } from "$lib/types";

type AgentConfigProps = {
  agent: AgentInfo;
  tab: AgentTabDto | null;
  projects?: ProjectDto[];
  selectedPath?: string | null;
  log?: TripEntry[];
  driftCount?: number;
  onSelectScope?: (path: string | null) => void;
  onPickFolder?: () => void;
};

function skillsValue(id: AgentId, rootPath: string): string {
  switch (id) {
    case "claude":
      return `${rootPath}/plugins · user skills`;
    case "antigravity":
      return `${rootPath}/antigravity-cli/skills · .agents/skills`;
    case "codex":
      return `${rootPath}/skills · ~/.agents/skills`;
    case "cursor":
      return `${rootPath}/skills · .cursor/skills`;
  }
}

function pluginToggleValue(id: AgentId): string {
  switch (id) {
    case "claude":
      return "claude plugin enable/disable";
    case "antigravity":
      return "agy plugin enable/disable";
    case "codex":
      return "file patch · [plugins.*.enabled]";
    case "cursor":
      return "not implemented yet";
  }
}

function mcpToggleValue(id: AgentId): string {
  switch (id) {
    case "claude":
      return "file patch · ~/.claude.json disabled";
    case "antigravity":
      return "file patch · mcpServers.*.disabled";
    case "codex":
      return "file patch · [mcp_servers.*.enabled]";
    case "cursor":
      return "managed in Cursor · read-only here";
  }
}

function pluginsDirValue(id: AgentId, rootPath: string): string {
  return id === "antigravity"
    ? `${rootPath}/config/plugins · antigravity-cli/plugins`
    : `${rootPath}/plugins`;
}

function cliBinaryName(id: AgentId): string {
  switch (id) {
    case "antigravity":
      return "agy";
    case "cursor":
      return "agent";
    default:
      return id;
  }
}

function Status({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span
      className={`shrink-0 border px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] ${
        ok ? "border-[var(--live)] text-[var(--live)]" : "border-[var(--trip)] text-[var(--trip)]"
      }`}
    >
      {label}
    </span>
  );
}

function PathRow({
  label,
  value,
  state,
  ok,
}: {
  label: string;
  value: string;
  state: string;
  ok: boolean;
}) {
  return (
    <div className="flex min-w-0 items-start gap-3 border-b border-[var(--hair)] px-3.5 py-2.5 last:border-b-0">
      <div className="w-[108px] shrink-0 pt-0.5 text-[10px] leading-snug font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
        {label}
      </div>
      <div className="min-w-0 flex-1 truncate font-mono text-[12px] leading-snug" title={value}>
        {value}
      </div>
      <Status ok={ok} label={state} />
    </div>
  );
}

function Capability({
  label,
  detail,
  ok,
}: {
  label: string;
  detail: string;
  ok: boolean;
}) {
  return (
    <div className="flex items-start gap-2.5 border-b border-[var(--hair)] py-2.5 last:border-b-0">
      <Status ok={ok} label={ok ? "YES" : "NO"} />
      <div className="min-w-0">
        <div className="text-[13px] font-semibold leading-none">{label}</div>
        <div className="mt-1 font-mono text-[11px] leading-snug text-[var(--mute)]">{detail}</div>
      </div>
    </div>
  );
}

export function AgentConfig({
  agent,
  tab,
  projects = [],
  selectedPath = null,
  log = [],
  driftCount = 0,
  onSelectScope,
  onPickFolder,
}: AgentConfigProps) {
  const [query, setQuery] = useState("");
  const root = agentRoot(agent.id);
  const pluginCount = tab?.plugins.length ?? 0;
  const skillCount =
    (tab?.plugins.reduce((sum, plugin) => sum + plugin.skills.length, 0) ?? 0) +
    (tab?.userSkills.length ?? 0);
  const mcpCount = tab?.mcpServers.length ?? 0;
  const installModes =
    [agent.installGit ? "git/url" : null, agent.installFolder ? "folder" : null].filter(Boolean).join(" · ") ||
    "unavailable";

  const pathRows = [
    {
      label: "CLI binary",
      value: cliBinaryName(agent.id),
      state: agent.cliOk ? "FOUND" : "MISSING",
      ok: agent.cliOk,
    },
    {
      label: "Global config",
      value: `${root}/`,
      state: agent.cliOk ? "REACHABLE" : "UNKNOWN",
      ok: agent.cliOk,
    },
    {
      label: "Plugins dir",
      value: pluginsDirValue(agent.id, root),
      state: `${pluginCount} FOUND`,
      ok: true,
    },
    {
      label: "Skills",
      value: skillsValue(agent.id, root),
      state: `${skillCount} SCANNED`,
      ok: true,
    },
    {
      label: "MCP config",
      value: mcpConfigPath(agent.id),
      state: tab ? `${mcpCount} FOUND` : "NOT READ",
      ok: Boolean(tab),
    },
    {
      label: "Backup dir",
      value: "~/.on-n-off/backups",
      state: "OK",
      ok: true,
    },
  ];

  const selected = selectedPath
    ? (projects.find((project) => sameProjectPath(project.path, selectedPath)) ?? null)
    : null;

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return projects;
    return projects.filter(
      (project) =>
        project.label.toLowerCase().includes(q) ||
        project.path.toLowerCase().includes(q) ||
        (project.branch ?? "").toLowerCase().includes(q),
    );
  }, [projects, query]);

  const withLocal = projects.filter((project) => (project.skillCount ?? 0) + (project.mcpCount ?? 0) > 0).length;
  const recent = log.slice(0, 6);

  return (
    <div className="flex flex-col gap-4 px-5 pt-[18px] pb-[26px]">
      <header className="flex flex-wrap items-end gap-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <ProviderIcon provider={agent.id} className="size-5 shrink-0" />
          <h2 className="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">
            {agent.displayName} config
          </h2>
        </div>
        <div className="flex-1" />
        <div className="flex flex-wrap items-center gap-2 font-mono text-[11px] text-[var(--mute)]">
          <Status ok={agent.cliOk} label={agent.cliOk ? "CLI OK" : "CLI DOWN"} />
          {driftCount > 0 ? (
            <span className="border border-[var(--warn)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--warn)]">
              {driftCount} DRIFT
            </span>
          ) : (
            <span className="border border-[var(--hair)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em]">
              IN SYNC
            </span>
          )}
        </div>
      </header>

      {agent.cliError ? (
        <p className="rounded-[11px] border border-[var(--trip)] bg-[var(--plate)] px-3.5 py-2.5 text-[12.5px] text-[var(--trip)]">
          {agent.cliError}
        </p>
      ) : null}

      <section className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Project scope">
        <header className="flex flex-wrap items-center gap-3 border-b border-[var(--hair)] px-3.5 py-3">
          <div className="min-w-0">
            <div className="text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
              Project scope on {agent.displayName}
            </div>
            <div className="mt-1 font-mono text-[11.5px] text-[var(--mute)]">
              {selectedPath ? "project mode" : "global config"} · {projects.length} recognized
              {withLocal > 0 ? ` · ${withLocal} with local skills/MCP` : ""}
            </div>
          </div>
          <div className="flex-1" />
          <label className="flex h-8 min-w-[180px] flex-1 items-center gap-2 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2.5 sm:max-w-[240px]">
            <Search className="size-3.5 shrink-0 text-[var(--mute)]" aria-hidden="true" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter projects…"
              className="min-w-0 flex-1 border-0 bg-transparent text-[12.5px] text-[var(--silkscreen)] outline-none placeholder:text-[var(--mute)]"
            />
          </label>
          {onPickFolder ? (
            <button
              type="button"
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[var(--hair)] bg-transparent px-2.5 text-[10.5px] font-semibold tracking-[0.04em] uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fill)]"
              onClick={onPickFolder}
            >
              <FolderPlus className="size-3.5" aria-hidden="true" />
              Folder
            </button>
          ) : null}
        </header>

        <div className="grid gap-3 border-b border-[var(--hair)] px-3.5 py-3 sm:grid-cols-3">
          <div className="rounded-md border border-[var(--hair)] bg-[var(--well)] px-3 py-2.5">
            <div className="text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">Active scope</div>
            <div className="mt-1 truncate text-[14px] font-semibold">{selected?.label ?? "All projects"}</div>
            <div className="mt-1 truncate font-mono text-[11px] text-[var(--mute)]">
              {selected?.path ?? `${root}/ · global source of truth`}
            </div>
          </div>
          <div className="rounded-md border border-[var(--hair)] bg-[var(--well)] px-3 py-2.5">
            <div className="text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">Local skills</div>
            <div className="mt-1 text-[22px] leading-none font-semibold">
              {selected ? (selected.skillCount ?? 0) : projects.reduce((n, p) => n + (p.skillCount ?? 0), 0)}
            </div>
            <div className="mt-1 text-[11.5px] text-[var(--mute)]">
              {selected ? "in the selected project" : "across recognized projects"}
            </div>
          </div>
          <div className="rounded-md border border-[var(--hair)] bg-[var(--well)] px-3 py-2.5">
            <div className="text-[10px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">Project MCP</div>
            <div className="mt-1 text-[22px] leading-none font-semibold">
              {selected ? (selected.mcpCount ?? 0) : projects.reduce((n, p) => n + (p.mcpCount ?? 0), 0)}
            </div>
            <div className="mt-1 text-[11.5px] text-[var(--mute)]">
              {selected ? "servers in this project" : "servers across projects"}
            </div>
          </div>
        </div>

        <p className="border-b border-[var(--hair)] px-3.5 py-2.5 text-[12px] text-[var(--mute)]">
          {selectedPath
            ? "Reading local skills/MCP from this folder. Presence is on; no project write yet."
            : "Global agent config is the source of truth. Pick a project to inspect local skills and MCP."}
        </p>

        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_72px_64px_64px] gap-2 border-b border-[var(--hair)] px-3.5 py-2 font-mono text-[10px] tracking-[0.04em] text-[var(--mute)] uppercase">
          <span>Project</span>
          <span>Path</span>
          <span>Branch</span>
          <span className="text-right">Skills</span>
          <span className="text-right">MCP</span>
        </div>

        <button
          type="button"
          className={`grid w-full cursor-pointer grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_72px_64px_64px] items-center gap-2 border-0 border-b border-[var(--hair)] px-3.5 py-2.5 text-left ${
            !selectedPath ? "bg-[var(--well)]" : "bg-transparent hover:bg-[var(--well)]"
          }`}
          onClick={() => onSelectScope?.(null)}
        >
          <span className="truncate text-[13.5px] font-semibold">All projects</span>
          <span className="truncate font-mono text-[11.5px] text-[var(--mute)]">{root}/</span>
          <span className="font-mono text-[11px] text-[var(--mute)]">—</span>
          <span className="font-mono text-right text-[12px]">
            {projects.reduce((n, p) => n + (p.skillCount ?? 0), 0)}
          </span>
          <span className="font-mono text-right text-[12px]">
            {projects.reduce((n, p) => n + (p.mcpCount ?? 0), 0)}
          </span>
        </button>

        {projects.length === 0 ? (
          <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">
            No recognized projects yet. Use Folder or SCOPE to add one.
          </p>
        ) : filtered.length === 0 ? (
          <p className="px-3.5 py-4 text-[13px] text-[var(--mute)]">No projects match “{query.trim()}”.</p>
        ) : (
          filtered.map((project) => {
            const active = selectedPath ? sameProjectPath(project.path, selectedPath) : false;
            return (
              <button
                key={project.id}
                type="button"
                className={`grid w-full cursor-pointer grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_72px_64px_64px] items-center gap-2 border-0 border-b border-[var(--hair)] px-3.5 py-2.5 text-left last:border-b-0 ${
                  active ? "bg-[var(--well)]" : "bg-transparent hover:bg-[var(--well)]"
                }`}
                title={project.path}
                onClick={() => onSelectScope?.(project.path)}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <span
                    className={`size-1.5 shrink-0 rounded-full ${active ? "bg-[var(--live)]" : "bg-[var(--mute)] opacity-40"}`}
                    aria-hidden="true"
                  />
                  <span className="truncate text-[13.5px] font-semibold">{project.label}</span>
                </span>
                <span className="truncate font-mono text-[11.5px] text-[var(--mute)]">{project.path}</span>
                <span className="truncate font-mono text-[11px] text-[var(--mute)]">{project.branch || "—"}</span>
                <span className="font-mono text-right text-[12px]">{project.skillCount ?? 0}</span>
                <span className="font-mono text-right text-[12px]">{project.mcpCount ?? 0}</span>
              </button>
            );
          })
        )}
      </section>

      <div className="grid gap-3 xl:grid-cols-[minmax(0,1.4fr)_minmax(280px,0.8fr)]">
        <section className="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Config paths">
          <header className="border-b border-[var(--hair)] px-3.5 py-2.5 text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Surfaces this switchboard reads
          </header>
          <div className="grid md:grid-cols-2 md:[&>*:nth-child(odd)]:border-r md:[&>*:nth-child(odd)]:border-[var(--hair)]">
            {pathRows.map((row) => (
              <PathRow key={row.label} {...row} />
            ))}
          </div>
        </section>

        <div className="grid gap-3">
          <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] px-3.5" aria-label="Adapter capabilities">
            <header className="border-b border-[var(--hair)] py-2.5 text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
              Adapter capabilities
            </header>
            <Capability label="Install from git/url" detail={installModes} ok={agent.installGit} />
            <Capability
              label="Install from folder"
              detail={agent.installFolder ? "supported" : "not on this CLI"}
              ok={agent.installFolder}
            />
            <Capability label="Plugin toggle" detail={pluginToggleValue(agent.id)} ok={agent.pluginToggle} />
            <Capability label="MCP toggle" detail={mcpToggleValue(agent.id)} ok={agent.id !== "cursor"} />
            <Capability label="Safe writes" detail="backup + rollback on every patch" ok />
          </section>

          <section className="flex flex-col gap-3 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5" aria-label="Write behaviour">
            <span className="text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
              Write behaviour
            </span>
            <div>
              <div className="text-[13px] font-semibold">Back up before write</div>
              <div className="mt-0.5 text-[11.5px] text-[var(--mute)]">
                Copy the target file into ~/.on-n-off/backups. Always on.
              </div>
            </div>
            <div>
              <div className="text-[13px] font-semibold">Roll back on failure</div>
              <div className="mt-0.5 text-[11.5px] text-[var(--mute)]">
                Restore the previous file if a write fails. Always on.
              </div>
            </div>
            <div>
              <div className="text-[13px] font-semibold">Watch config files</div>
              <div className="mt-0.5 text-[11.5px] text-[var(--mute)]">Not in v1. Refresh still rescans.</div>
            </div>
          </section>
        </div>
      </div>

      <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label="Recent trips">
        <header className="flex items-center gap-2 border-b border-[var(--hair)] px-3.5 py-2.5">
          <span className="text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Recent trips
          </span>
          <div className="flex-1" />
          <Link
            to="/overview"
            className="text-[11px] font-semibold tracking-[0.04em] text-[var(--mute)] uppercase no-underline hover:text-[var(--silkscreen)]"
          >
            Overview
          </Link>
        </header>
        {recent.length === 0 ? (
          <p className="px-3.5 py-3 text-[13px] text-[var(--mute)]">
            Installs, toggles, and syncs for this session will land here.
          </p>
        ) : (
          recent.map((entry, index) => (
            <div
              key={`${entry.at}-${entry.tag}-${index}`}
              className="flex items-center gap-2.5 border-b border-[var(--hair)] px-3.5 py-2 last:border-b-0"
            >
              <span className="font-mono w-10 shrink-0 text-[11px] text-[var(--mute)]">{entry.at}</span>
              <span
                className={`shrink-0 px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] ${tripTagClass(entry.tag)}`}
              >
                {entry.tag}
              </span>
              <span className="min-w-0 truncate text-[12.5px]">{entry.text}</span>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
