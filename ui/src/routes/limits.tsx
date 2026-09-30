import { Limits } from "@/features/limits/Limits";
import { useAgentSession } from "@/features/session/SessionProvider";

export function LimitsRoute() {
  const session = useAgentSession();
  return (
    <Limits
      pollMinutes={session.appSettings.limitsPollMinutes}
      resetAlerts={session.appSettings.resetAlerts}
      onResetAlertsChange={async (resetAlerts) => {
        // The session reports a failed save as a note of its own and answers null.
        if (!(await session.persistAppSettings({ ...session.appSettings, resetAlerts }))) {
          throw new Error("The banked reset alert was not saved.");
        }
      }}
    />
  );
}
