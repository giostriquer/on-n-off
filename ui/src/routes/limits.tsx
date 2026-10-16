import { Limits } from "@/features/limits/Limits";
import { useAppSettings } from "$lib/useAppSettings";

export function LimitsRoute() {
  const { settings, patch } = useAppSettings();
  return (
    <Limits
      pollMinutes={settings.limitsPollMinutes}
      resetAlerts={settings.resetAlerts}
      onResetAlertsChange={async (resetAlerts) => {
        if (!(await patch({ resetAlerts }))) {
          throw new Error("The banked reset alert was not saved.");
        }
      }}
    />
  );
}
