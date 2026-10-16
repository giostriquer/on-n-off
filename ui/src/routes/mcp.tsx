import { McpList } from "@/features/catalog/McpList";
import { useAgentSession } from "@/features/session/SessionProvider";
import { mcpReadOnlyNotice } from "$lib/providers";

export function McpRoute() {
  const session = useAgentSession();
  return (
    <McpList
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      servers={session.filtered?.mcpServers ?? []}
      filterQuery={session.currentTab.filter}
      busy={session.currentTab.inFlight}
      notice={mcpReadOnlyNotice(session.currentAgent.id) ?? undefined}
      onToggle={session.toggleMcp}
    />
  );
}
