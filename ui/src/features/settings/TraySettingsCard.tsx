import { useQuery } from "@tanstack/react-query";
import { Rocker } from "@/features/agents/Rocker";
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
    <section
      aria-label="Windows tray"
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
    >
      <div className="flex flex-wrap items-start gap-3 px-3.5 py-3">
        <div className="min-w-0 flex-1">
          <h3 className="m-0 text-[15px] font-semibold">Windows tray</h3>
          <p className="mt-1 mb-0 text-[11.5px] text-[var(--mute)]">
            on-n-off always keeps an icon in the notification area. Turn this on and closing the
            window leaves it running there instead of quitting.
          </p>
        </div>
        <div className="flex flex-col items-end gap-1">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Close to tray
          </span>
          <Rocker
            size="skill"
            on={closeToTray}
            ariaLabel="Keep on-n-off running in the tray when the window is closed"
            onToggle={() => onCloseToTrayChange(!closeToTray)}
          />
        </div>
      </div>
    </section>
  );
}
