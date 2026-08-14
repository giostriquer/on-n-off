import { AgentConfig } from "@/features/agents/AgentConfig";
import { useAgentSession } from "@/features/session/SessionProvider";

export function ConfigRoute() {
  const session = useAgentSession();
  return (
    <AgentConfig
      agent={session.currentAgent}
      tab={session.currentTab.dto}
      projects={session.currentProjects}
      selectedPath={session.currentScopePath}
      log={session.log}
      driftCount={session.drift.length}
      onSelectScope={(path) => {
        void session.selectScope(path);
      }}
      onPickFolder={() => {
        void session.pickProjectFolder();
      }}
    />
  );
}
