import { McpList } from "@/features/catalog/McpList";
import { useAgentSession } from "@/features/session/SessionProvider";
import { providerToggles } from "$lib/providers";

export function McpRoute() {
  const session = useAgentSession();
  const { mcp } = providerToggles(session.currentAgent.id);
  return (
    <McpList
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      servers={session.filtered?.mcpServers ?? []}
      filterQuery={session.currentTab.filter}
      busy={session.currentTab.inFlight}
      notice={mcp.toggle ? undefined : mcp.notice}
      onToggle={session.toggleMcp}
    />
  );
}
