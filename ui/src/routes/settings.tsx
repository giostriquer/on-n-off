import { Settings } from "@/features/settings/Settings";
import { useAgentSession } from "@/features/session/SessionProvider";
import { setAgentHidden, withBinaryPath } from "$lib/appSettings";
import { useAppSettings } from "$lib/useAppSettings";
import type { AgentId } from "$lib/types";

export function SettingsRoute() {
  const session = useAgentSession();
  const { settings, patch } = useAppSettings();

  async function saveBinary(id: AgentId, path: string) {
    if (await patch((latest) => ({ binaryPaths: withBinaryPath(latest.binaryPaths, id, path) }))) {
      await session.reloadAgents();
    }
  }

  return (
    <Settings
      agents={session.agents}
      settings={settings}
      onToggleVisible={(id, hidden) => {
        void patch((latest) => ({ hiddenAgents: setAgentHidden(latest.hiddenAgents, id, hidden) }));
      }}
      onSaveBinary={(id, path) => {
        void saveBinary(id, path);
      }}
      onSettingsChange={(change) => {
        void patch(change);
      }}
    />
  );
}
