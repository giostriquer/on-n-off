import { ItemList } from "@/features/catalog/ItemList";
import { useAgentSession } from "@/features/session/SessionProvider";

export function PluginsRoute() {
  const session = useAgentSession();
  return (
    <ItemList
      kind="plugin"
      tab={{
        plugins: session.filtered?.plugins ?? [],
        userSkills: [],
        mcpServers: session.filtered?.mcpServers ?? [],
      }}
      filterQuery={session.currentTab.filter}
      expandedIds={session.expandedIds}
      cliOk={session.currentAgent.cliOk}
      pluginToggle={session.currentAgent.pluginToggle}
      busy={session.currentTab.inFlight}
      onToggleExpand={session.toggleExpand}
      onTogglePlugin={session.togglePlugin}
      onToggleSkill={session.toggleSkill}
      onUninstall={(plugin) => session.setUninstallTarget(plugin)}
      onUpdate={session.updatePlugin}
    />
  );
}
