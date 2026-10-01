import { useQuery } from "@tanstack/react-query";
import { CardToggle, SettingsCard } from "@/components/SettingsCard";
import * as api from "$lib/api";

type TraySettingsCardProps = {
  closeToTray: boolean;
  onCloseToTrayChange: (enabled: boolean) => void;
};

export function TraySettingsCard({ closeToTray, onCloseToTrayChange }: TraySettingsCardProps) {
  const supported = useQuery({
    queryKey: ["tray-supported"],
    queryFn: () => api.traySupported(),
    staleTime: Infinity,
  });

  if (supported.data !== true) return null;

  return (
    <SettingsCard
      title="Windows tray"
      description="on-n-off always keeps an icon in the notification area. Turn this on and closing the window leaves it running there instead of quitting."
      control={
        <CardToggle
          caption="Close to tray"
          on={closeToTray}
          ariaLabel="Keep on-n-off running in the tray when the window is closed"
          onToggle={() => onCloseToTrayChange(!closeToTray)}
        />
      }
    />
  );
}
