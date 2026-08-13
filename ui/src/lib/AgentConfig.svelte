<script lang="ts">
  import { agentRoot, mcpConfigPath } from "./catalog";
  import type { AgentId, AgentInfo, AgentTabDto, ProjectDto } from "./types";

  let {
    agent,
    tab,
    projects = [],
    selectedPath = null,
  }: {
    agent: AgentInfo;
    tab: AgentTabDto | null;
    projects?: ProjectDto[];
    selectedPath?: string | null;
  } = $props();

  const root = $derived(agentRoot(agent.id));
  const pluginCount = $derived(tab?.plugins.length ?? 0);
  const skillCount = $derived(
    (tab?.plugins.reduce((sum, plugin) => sum + plugin.skills.length, 0) ?? 0) + (tab?.userSkills.length ?? 0),
  );

  function skillsValue(id: AgentId, rootPath: string): string {
    switch (id) {
      case "claude":
        return `${rootPath}/plugins · user skills`;
      case "antigravity":
        return `${rootPath}/antigravity-cli/skills · .agents/skills`;
      case "codex":
        return `${rootPath}/skills · ~/.agents/skills`;
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
    }
  }

  function pluginsDirValue(id: AgentId, rootPath: string): string {
    return id === "antigravity"
      ? `${rootPath}/config/plugins · antigravity-cli/plugins`
      : `${rootPath}/plugins`;
  }

  const rows = $derived([
    {
      label: "CLI binary",
      value: agent.id === "antigravity" ? "agy" : agent.id,
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
      state: tab ? `${tab.mcpServers.length} FOUND` : "NOT READ",
      ok: Boolean(tab),
    },
    {
      label: "Backup dir",
      value: "~/.on-n-off/backups",
      state: "OK",
      ok: true,
    },
    {
      label: "Plugin toggle",
      value: pluginToggleValue(agent.id),
      state: agent.pluginToggle ? "SUPPORTED" : "BLOCKED",
      ok: agent.pluginToggle,
    },
    {
      label: "MCP toggle",
      value: mcpToggleValue(agent.id),
      state: "SUPPORTED",
      ok: true,
    },
    {
      label: "Install",
      value: [agent.installGit ? "git/url" : null, agent.installFolder ? "folder" : null].filter(Boolean).join(" · ") || "unavailable",
      state: agent.installGit ? "SUPPORTED" : "BLOCKED",
      ok: agent.installGit,
    },
  ]);
</script>

<div class="flex max-w-[820px] flex-col gap-3.5 px-5 py-[18px]">
  <h2 class="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">{agent.displayName} config</h2>
  <div class="overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
    {#each rows as row (row.label)}
      <div class="flex items-center gap-3.5 border-b border-[var(--hair)] px-3.5 py-[11px] last:border-b-0">
        <div class="w-[170px] shrink-0 text-[10.5px] leading-snug font-semibold tracking-[0.04em] text-[var(--mute)] uppercase">
          {row.label}
        </div>
        <div class="min-w-0 flex-1 truncate font-mono text-[12.5px] leading-snug">{row.value}</div>
        <span
          class="shrink-0 border px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] {row.ok
            ? 'border-[var(--live)] text-[var(--live)]'
            : 'border-[var(--trip)] text-[var(--trip)]'}"
        >
          {row.state}
        </span>
      </div>
    {/each}
  </div>
  <div class="grid grid-cols-2 gap-3">
    <section class="flex flex-col gap-2.5 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5">
      <span class="text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">Write behaviour</span>
      <div>
        <div class="text-[13px] font-semibold">Back up before write</div>
        <div class="text-[11.5px] text-[var(--mute)]">Copy the config file into ~/.on-n-off/backups. Always on.</div>
      </div>
      <div>
        <div class="text-[13px] font-semibold">Roll back on failure</div>
        <div class="text-[11.5px] text-[var(--mute)]">Restore the previous file if a write fails. Always on.</div>
      </div>
      <div>
        <div class="text-[13px] font-semibold">Watch config files</div>
        <div class="text-[11.5px] text-[var(--mute)]">Not in v1. Refresh still rescans.</div>
      </div>
    </section>
    <section class="flex flex-col gap-2 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-3.5">
      <span class="text-[11px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase"
        >Project scope on {agent.displayName}</span
      >
      {#if projects.length === 0 && !selectedPath}
        <span class="text-xs text-[var(--mute)]">No recognized projects yet. Pick a folder from SCOPE to scan local skills.</span>
      {:else}
        <span class="text-xs text-[var(--mute)]">
          {selectedPath
            ? `Reading local skills/MCP from ${selectedPath}. Presence is on; no project write yet.`
            : `${projects.length} recognized ${projects.length === 1 ? "project" : "projects"} in ${agent.displayName} config.`}
        </span>
        <div class="flex max-h-28 flex-col gap-1 overflow-y-auto font-mono text-[11.5px] text-[var(--mute)]">
          {#each projects as project (project.id)}
            <span title={project.path}>{project.label}</span>
          {/each}
        </div>
      {/if}
    </section>
  </div>
</div>
