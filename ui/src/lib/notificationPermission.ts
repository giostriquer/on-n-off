import * as api from "./api";

/**
 * Asks the OS to let on-n-off show notifications, for a setting that only notifies: `null` once it
 * may, otherwise why it may not, for the setting to show while it stays off.
 */
export async function notificationPermissionProblem(): Promise<string | null> {
  try {
    return (await api.requestNotificationPermission()) ? null : "Notifications are blocked in system settings.";
  } catch {
    return "Could not request notification permission.";
  }
}
