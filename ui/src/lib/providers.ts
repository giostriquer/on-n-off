import catalogue from "../../../src-tauri/src/provider_catalogue/fixtures/providers.json";
import type { AgentId } from "./types";

export type LimitsProvider = keyof typeof catalogue.withLimits;
export type AccountProvider = keyof typeof catalogue.withSavedAccounts;

type CatalogueRow = (typeof catalogue.providers)[AgentId];

const ROWS: Readonly<Record<AgentId, CatalogueRow>> = catalogue.providers;

export function isAgentId(value: unknown): value is AgentId {
  return typeof value === "string" && Object.hasOwn(ROWS, value);
}

export function hasLimits(id: AgentId): id is LimitsProvider {
  return Object.hasOwn(catalogue.withLimits, id);
}

export function hasSavedAccounts(id: AgentId): id is AccountProvider {
  return Object.hasOwn(catalogue.withSavedAccounts, id);
}

export const PROVIDER_IDS: readonly AgentId[] = catalogue.tabOrder.filter(isAgentId);

export const RAIL_ORDER: readonly AgentId[] = catalogue.railOrder.filter(isAgentId);

export const LIMITS_PROVIDERS: readonly LimitsProvider[] = PROVIDER_IDS.filter(hasLimits);

export const ACCOUNT_PROVIDERS: readonly AccountProvider[] = PROVIDER_IDS.filter(hasSavedAccounts);

export const USAGE_PROVIDERS: readonly AgentId[] = PROVIDER_IDS.filter((id) => ROWS[id].usage !== null);

export function providerLabel(id: AgentId): string {
  return ROWS[id].displayName;
}

export function providerBinary(id: AgentId): string {
  return ROWS[id].binary;
}

export function providerNames(ids: readonly AgentId[]): string {
  const names = ids.map(providerLabel);
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} or ${names.at(-1)}` : names.join("");
}

export function providerRecord<T>(value: (id: AgentId) => T): Record<AgentId, T> {
  return Object.fromEntries(PROVIDER_IDS.map((id) => [id, value(id)])) as Record<AgentId, T>;
}

type ProviderLook = {
  readonly icon: string;
  readonly bar: string;
  readonly chart: { readonly light: string; readonly dark: string };
};

const LOOK: Readonly<Record<AgentId, ProviderLook>> = {
  claude: { icon: "#d97757", bar: "#d97757", chart: { light: "#d9773a", dark: "#d97757" } },
  codex: { icon: "var(--silkscreen)", bar: "var(--silkscreen)", chart: { light: "#2a2a2a", dark: "#ececec" } },
  antigravity: { icon: "#3186ff", bar: "var(--mute)", chart: { light: "#6c727c", dark: "#8c939d" } },
  cursor: { icon: "var(--silkscreen)", bar: "#7aa2ff", chart: { light: "#3b6dff", dark: "#7aa2ff" } },
  muse: { icon: "#0082fb", bar: "#0082fb", chart: { light: "#0082fb", dark: "#0082fb" } },
};

export function providerColor(id: AgentId): string {
  return LOOK[id].bar;
}

export function providerIconColor(id: AgentId): string {
  return LOOK[id].icon;
}

export function providerChartColor(id: AgentId, dark: boolean): string {
  return LOOK[id].chart[dark ? "dark" : "light"];
}

export type McpToggle =
  | { readonly toggle: true; readonly detail: string }
  | { readonly toggle: false; readonly detail: string; readonly notice: string };

export type ProviderToggles = {
  readonly plugin: string;
  readonly mcp: McpToggle;
};

const TOGGLES: Readonly<Record<AgentId, ProviderToggles>> = {
  claude: {
    plugin: "claude plugin enable/disable",
    mcp: { toggle: true, detail: "file patch · ~/.claude.json disabled" },
  },
  codex: {
    plugin: "file patch · [plugins.*.enabled]",
    mcp: { toggle: true, detail: "file patch · [mcp_servers.*.enabled]" },
  },
  antigravity: {
    plugin: "agy plugin enable/disable",
    mcp: { toggle: true, detail: "file patch · mcpServers.*.disabled" },
  },
  cursor: {
    plugin: "not implemented yet",
    mcp: {
      toggle: false,
      detail: "managed in Cursor · read-only here",
      notice:
        "Cursor keeps MCP servers on or off in its own settings, so on-n-off lists them but cannot switch them here. Manage them in the Cursor app, or run `agent mcp enable <name>` / `agent mcp disable <name>`.",
    },
  },
  muse: {
    plugin: "muse plugins enable/disable",
    mcp: {
      toggle: false,
      detail: "managed in Muse · read-only here",
      notice:
        "Muse manages MCP servers itself, so on-n-off lists them but cannot switch them here. Edit `mcpServers` in Muse’s settings.json, or use `muse mcp login <name>`.",
    },
  },
};

export function providerToggles(id: AgentId): ProviderToggles {
  return TOGGLES[id];
}

export type ProviderPaths = {
  readonly home: string;
  readonly pluginDirs: string;
  readonly skillDirs: string;
  readonly mcpConfig: string;
  readonly scopeConfig: string;
  readonly userSkills: string;
  readonly projectSkills: string;
};

const PATHS: Readonly<Record<AgentId, ProviderPaths>> = {
  claude: {
    home: "~/.claude",
    pluginDirs: "~/.claude/plugins",
    skillDirs: "~/.claude/plugins · user skills",
    mcpConfig: "~/.claude.json",
    scopeConfig: "~/.claude/settings.json",
    userSkills: "~/.claude/skills",
    projectSkills: ".claude/skills",
  },
  codex: {
    home: "~/.codex",
    pluginDirs: "~/.codex/plugins",
    skillDirs: "~/.codex/skills · ~/.agents/skills",
    mcpConfig: "~/.codex/config.toml",
    scopeConfig: "~/.codex/config.toml",
    userSkills: "~/.codex/skills",
    projectSkills: ".codex/skills",
  },
  antigravity: {
    home: "~/.gemini",
    pluginDirs: "~/.gemini/config/plugins · antigravity-cli/plugins",
    skillDirs: "~/.gemini/antigravity-cli/skills · .agents/skills",
    mcpConfig: "~/.gemini/config/mcp_config.json",
    scopeConfig: "~/.gemini/antigravity-cli/",
    userSkills: "~/.gemini/antigravity-cli/skills",
    projectSkills: ".agents/skills",
  },
  cursor: {
    home: "~/.cursor",
    pluginDirs: "~/.cursor/plugins",
    skillDirs: "~/.cursor/skills · .cursor/skills",
    mcpConfig: "~/.cursor/mcp.json",
    scopeConfig: "~/.cursor/",
    userSkills: "~/.cursor/skills",
    projectSkills: ".cursor/skills",
  },
  muse: {
    home: "~/.local/share/muse",
    pluginDirs: "~/.local/share/muse/plugins",
    skillDirs: "~/.config/muse/skills · ~/.agents/skills",
    mcpConfig: "~/.config/muse/settings.json",
    scopeConfig: "~/.config/muse/settings.json",
    userSkills: "~/.config/muse/skills",
    projectSkills: ".agents/skills",
  },
};

export function providerPaths(id: AgentId): ProviderPaths {
  return PATHS[id];
}
