import { Overview } from "@/features/catalog/Overview";
import { useAgentSession } from "@/features/session/SessionProvider";

export function OverviewRoute() {
  const session = useAgentSession();
  return (
    <Overview
      counts={session.counts}
      rows={session.live}
      drift={session.drift}
      log={session.log}
      busy={session.currentTab.inFlight}
      pluginToggle={session.currentAgent.pluginToggle}
      scopeLabel={session.currentScopeLabel}
      onToggle={session.toggleLive}
      onUpdate={(pluginId) => {
        const plugin = session.currentTab.dto?.plugins.find((item) => item.id === pluginId);
        if (plugin) {
          session.updatePlugin(plugin);
        }
      }}
      cliOk={session.currentAgent.cliOk}
    />
  );
}
