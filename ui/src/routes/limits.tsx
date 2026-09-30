import { Limits } from "@/features/limits/Limits";
import { useAgentSession } from "@/features/session/SessionProvider";

export function LimitsRoute() {
  const session = useAgentSession();
  return (
    <Limits
      pollMinutes={session.appSettings.limitsPollMinutes}
      resetAlerts={session.appSettings.resetAlerts}
      onResetAlertsChange={async (resetAlerts) => {
        await session.persistAppSettings({ ...session.appSettings, resetAlerts });
      }}
    />
  );
}
