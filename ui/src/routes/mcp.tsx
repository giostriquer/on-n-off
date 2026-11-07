import { McpList } from "@/features/catalog/McpList";
import { useAgentSession } from "@/features/session/SessionProvider";
import { mcpNotice } from "$lib/capabilities";

export function McpRoute() {
  const session = useAgentSession();
  return (
    <McpList
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      servers={session.filtered?.mcpServers ?? []}
      filterQuery={session.currentTab.filter}
      busy={session.currentTab.inFlight}
      notice={mcpNotice(session.currentAgent, session.currentScopePath) ?? undefined}
      onToggle={session.toggleMcp}
    />
  );
}
