import { invoke } from "@tauri-apps/api/core";
import type { AgentId, AgentInfo, AgentTabDto, AppSettings, FeatureFlags, ProjectDto, ProviderDiagnose } from "./types";
import type { UsageSummary, UsageSummaryInput } from "./usageTypes";

export function listAgents(): Promise<AgentInfo[]> {
  return invoke("list_agents");
}

export function featureFlags(): Promise<FeatureFlags> {
  return invoke("feature_flags");
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

export function loadAppSettings(): Promise<AppSettings> {
  return invoke("load_app_settings");
}

export function saveAppSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke("save_app_settings", { settings });
}

export function diagnoseProviders(): Promise<ProviderDiagnose[]> {
  return invoke("diagnose_providers");
}
