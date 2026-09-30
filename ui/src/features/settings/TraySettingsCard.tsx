import { useQuery } from "@tanstack/react-query";
import { CardToggle, SettingsCard } from "@/components/SettingsCard";
import * as api from "$lib/api";

type TraySettingsCardProps = {
  closeToTray: boolean;
  onCloseToTrayChange: (enabled: boolean) => void;
};

/**
 * The Windows notification-area icon and what the close button does. macOS has a status item
 * too, but it is the Limits popover and it always hides on close, so this card renders only
 * where the setting means something.
 */
export function TraySettingsCard({ closeToTray, onCloseToTrayChange }: TraySettingsCardProps) {
  const supported = useQuery({
    queryKey: ["tray-supported"],
    queryFn: () => api.traySupported(),
    staleTime: Infinity,
  });

  if (supported.data !== true) return null;

  return (
    <SettingsCard
      label="Windows tray"
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
