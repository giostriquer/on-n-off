import { McpList } from "@/features/catalog/McpList";
import { useAgentSession } from "@/features/session/SessionProvider";
import { copy } from "$lib/copy";

export function McpRoute() {
  const session = useAgentSession();
  return (
    <McpList
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      servers={session.filtered?.mcpServers ?? []}
      filterQuery={session.currentTab.filter}
      busy={session.currentTab.inFlight}
      notice={session.currentAgent.id === "cursor" ? copy.cursorMcpReadOnly : undefined}
      onToggle={session.toggleMcp}
    />
  );
}
