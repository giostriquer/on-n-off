import { McpList } from "@/features/catalog/McpList";
import { useAgentSession } from "@/features/session/SessionProvider";

export function McpRoute() {
  const session = useAgentSession();
  return (
    <McpList
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      servers={session.filtered?.mcpServers ?? []}
      filterQuery={session.currentTab.filter}
      busy={session.currentTab.inFlight}
      onToggle={session.toggleMcp}
    />
  );
}
