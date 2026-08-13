import { invoke } from "@tauri-apps/api/core";
import type { AgentId, AgentInfo, AgentTabDto } from "./types";

export function listAgents(): Promise<AgentInfo[]> {
  return invoke("list_agents");
}

export function listPlugins(agentId: AgentId): Promise<AgentTabDto> {
  return invoke("list_plugins", { agent_id: agentId });
}

export function setPluginEnabled(
  agentId: AgentId,
  pluginId: string,
  enabled: boolean,
): Promise<AgentTabDto> {
  return invoke("set_plugin_enabled", { agent_id: agentId, plugin_id: pluginId, enabled });
}

export function setSkillEnabled(
  agentId: AgentId,
  skillId: string,
  enabled: boolean,
): Promise<AgentTabDto> {
  return invoke("set_skill_enabled", { agent_id: agentId, skill_id: skillId, enabled });
}

export function installPlugin(agentId: AgentId, source: string): Promise<AgentTabDto> {
  return invoke("install_plugin", { agent_id: agentId, source });
}

export function uninstallPlugin(agentId: AgentId, pluginId: string): Promise<AgentTabDto> {
  return invoke("uninstall_plugin", { agent_id: agentId, plugin_id: pluginId });
}

export function refresh(agentId: AgentId): Promise<AgentTabDto> {
  return invoke("refresh", { agent_id: agentId });
}
