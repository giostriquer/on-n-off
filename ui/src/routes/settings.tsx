import { Settings } from "@/features/settings/Settings";
import { useAgentSession } from "@/features/session/SessionProvider";

export function SettingsRoute() {
  const session = useAgentSession();
  return (
    <Settings
      agents={session.agents}
      settings={session.appSettings}
      onToggleVisible={(id, hidden) => {
        void session.setProviderHidden(id, hidden);
      }}
      onSaveBinary={(id, path) => {
        void session.setProviderBinary(id, path);
      }}
      onAutomaticUpdatesChange={(enabled) => {
        void session.persistAppSettings({ ...session.appSettings, automaticUpdates: enabled });
      }}
      onLimitNotificationsChange={(enabled) => {
        void session.persistAppSettings({
          ...session.appSettings,
          limitNotifications: enabled,
        });
      }}
      onLimitsPollMinutesChange={(minutes) => {
        void session.persistAppSettings({ ...session.appSettings, limitsPollMinutes: minutes });
      }}
      onSettingsChange={(patch) => {
        void session.persistAppSettings({ ...session.appSettings, ...patch });
      }}
    />
  );
}
