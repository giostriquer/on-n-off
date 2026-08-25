import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentId,
  AgentInfo,
  AgentTabDto,
  AppSettings,
  FeatureFlags,
  InstallItemsRequest,
  InstallItemsResult,
  ItemStatus,
  MarketplaceInspect,
  ProjectDto,
  ProviderDiagnose,
  UpdateItemMode,
  UpdaterBuildInfo,
} from "./types";
import type { GithubPrs } from "./githubTypes";
import type { ProviderLimits } from "./limitsTypes";
import type { UsageSummary, UsageSummaryInput } from "./usageTypes";

export function listAgents(): Promise<AgentInfo[]> {
  return invoke("list_agents");
}

export function featureFlags(): Promise<FeatureFlags> {
  return invoke("feature_flags");
}

export function updaterBuildInfo(): Promise<UpdaterBuildInfo> {
  return invoke("updater_build_info");
}

export function listProjects(agentId: AgentId): Promise<ProjectDto[]> {
  return invoke("list_projects", { agentId });
}

export function inspectProject(agentId: AgentId, path: string): Promise<ProjectDto> {
  return invoke("inspect_project", { agentId, path });
}

export function listPlugins(agentId: AgentId, projectPath?: string | null): Promise<AgentTabDto> {
  return invoke("list_plugins", { agentId, projectPath: projectPath || null });
}

export function listLocalPlugins(agentId: AgentId, projectPath?: string | null): Promise<AgentTabDto> {
  return invoke("list_local_plugins", { agentId, projectPath: projectPath || null });
}

export function setPluginEnabled(
  agentId: AgentId,
  pluginId: string,
  enabled: boolean,
): Promise<AgentTabDto> {
  return invoke("set_plugin_enabled", { agentId, pluginId, enabled });
}

export function setSkillEnabled(
  agentId: AgentId,
  skillId: string,
  enabled: boolean,
): Promise<AgentTabDto> {
  return invoke("set_skill_enabled", { agentId, skillId, enabled });
}

export function setMcpEnabled(
  agentId: AgentId,
  mcpId: string,
  enabled: boolean,
): Promise<AgentTabDto> {
  return invoke("set_mcp_enabled", { agentId, mcpId, enabled });
}

export function installPlugin(agentId: AgentId, source: string): Promise<AgentTabDto> {
  return invoke("install_plugin", { agentId, source });
}

export function inspectMarketplace(
  owner: string,
  repo: string,
  gitRef?: string | null,
): Promise<MarketplaceInspect> {
  return invoke("inspect_marketplace", { owner, repo, gitRef: gitRef || null });
}

export function installItems(request: InstallItemsRequest): Promise<InstallItemsResult> {
  return invoke("install_items", { request });
}

export function itemUpdateStatus(
  provider: AgentId,
  projectPath?: string | null,
  force = false,
): Promise<ItemStatus[]> {
  return invoke("item_update_status", { provider, projectPath: projectPath || null, force });
}

export function updateItem(id: string, mode: UpdateItemMode): Promise<ItemStatus> {
  return invoke("update_item", { id, mode });
}

export function removeItem(id: string): Promise<void> {
  return invoke("remove_item", { id });
}

/** Opens a github.com page in the default browser (the backend refuses anything else). */
export function openUrl(url: string): Promise<void> {
  return invoke("open_url", { url });
}

export function uninstallPlugin(agentId: AgentId, pluginId: string): Promise<AgentTabDto> {
  return invoke("uninstall_plugin", { agentId, pluginId });
}

export function updatePlugin(agentId: AgentId, pluginId: string): Promise<AgentTabDto> {
  return invoke("update_plugin", { agentId, pluginId });
}

export function refresh(agentId: AgentId, projectPath?: string | null): Promise<AgentTabDto> {
  return invoke("refresh", { agentId, projectPath: projectPath || null });
}

export function usageSummary(input: UsageSummaryInput): Promise<UsageSummary> {
  return invoke("usage_summary", { input });
}

/** Live limits for the signed-in account first, then remembered snapshots of other accounts. */
export function readLimits(agentId: AgentId, force = false): Promise<ProviderLimits[]> {
  return invoke("read_limits", { agentId, force });
}

export function forgetLimitsSnapshot(agentId: AgentId, accountId: string): Promise<void> {
  return invoke("forget_limits_snapshot", { agentId, accountId });
}

/** The GitHub screen's pull requests; `force` skips the backend's in-memory result. */
export function readGithubPrs(force = false): Promise<GithubPrs> {
  return invoke("read_github_prs", { force });
}

export function hideLimitsPopover(): Promise<void> {
  return invoke("hide_limits_popover");
}

export function openLimitsWindow(): Promise<void> {
  return invoke("open_limits_window");
}

export function quitApp(): Promise<void> {
  return invoke("quit_app");
}

export function onLimitsPopoverOpened(handler: () => void): Promise<UnlistenFn> {
  return listen("limits-popover-opened", () => handler());
}

export function onOpenLimitsWindow(handler: () => void): Promise<UnlistenFn> {
  return listen("open-limits-window", () => handler());
}

export function loadAppSettings(): Promise<AppSettings> {
  return invoke("load_app_settings");
}

export function saveAppSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke("save_app_settings", { settings });
}

export function requestNotificationPermission(): Promise<boolean> {
  return invoke("request_notification_permission");
}

export function diagnoseProviders(): Promise<ProviderDiagnose[]> {
  return invoke("diagnose_providers");
}
