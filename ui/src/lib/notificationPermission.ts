import * as api from "./api";

export async function notificationPermissionProblem(): Promise<string | null> {
  try {
    return (await api.requestNotificationPermission()) ? null : "Notifications are blocked in system settings.";
  } catch {
    return "Could not request notification permission.";
  }
}
