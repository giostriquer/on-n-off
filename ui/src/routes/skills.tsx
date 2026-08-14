import { ItemList } from "@/features/catalog/ItemList";
import { useAgentSession } from "@/features/session/SessionProvider";

export function SkillsRoute() {
  const session = useAgentSession();
  return (
    <ItemList
      kind="skill"
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      filterQuery={session.currentTab.filter}
      expandedIds={session.expandedIds}
      cliOk={session.currentAgent.cliOk}
      pluginToggle={session.currentAgent.pluginToggle}
      busy={session.currentTab.inFlight}
      onToggleExpand={session.toggleExpand}
      onTogglePlugin={session.togglePlugin}
      onToggleSkill={session.toggleSkill}
      onUninstall={(plugin) => session.setUninstallTarget(plugin)}
    />
  );
}
