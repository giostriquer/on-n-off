import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import type { AppSettings, ResetAlert } from "$lib/types";
import { AppSettingsProvider } from "$lib/useAppSettings";
import { LimitsRoute } from "./limits";

const LOADED: AppSettings = { ...DEFAULT_APP_SETTINGS, githubScopes: ["org:acme"], limitsPollMinutes: 10 };

const api = vi.hoisted(() => ({
  loadAppSettings: vi.fn<() => Promise<AppSettings>>(),
  saveAppSettings: vi.fn<(settings: AppSettings) => Promise<AppSettings>>(),
}));
const limitsProps = vi.hoisted(() => ({ current: null as null | {
  pollMinutes: number;
  resetAlerts: Record<string, ResetAlert>;
  onResetAlertsChange: (alerts: Record<string, ResetAlert>) => Promise<void>;
} }));

vi.mock("$lib/api", () => api);

vi.mock("@/features/limits/Limits", () => ({
  Limits: (props: NonNullable<typeof limitsProps.current>) => {
    limitsProps.current = props;
    return null;
  },
}));

const alert: ResetAlert = { label: "you@example.com", maxLeftPercent: 5, minHoursToRenewal: 48, automatic: false };

async function onResetAlertsChange() {
  render(
    <AppSettingsProvider>
      <LimitsRoute />
    </AppSettingsProvider>,
  );
  await waitFor(() => expect(limitsProps.current?.pollMinutes).toBe(10));
  return limitsProps.current!.onResetAlertsChange;
}

beforeEach(() => {
  api.loadAppSettings.mockReset().mockResolvedValue(LOADED);
  api.saveAppSettings.mockReset().mockImplementation(async (settings) => settings);
  limitsProps.current = null;
});

describe("the Limits route's banked reset alerts", () => {
  it("saves a change with the rest of the app's settings", async () => {
    await (await onResetAlertsChange())({ "acct-work": alert });

    expect(api.saveAppSettings).toHaveBeenCalledWith({
      ...DEFAULT_APP_SETTINGS,
      githubScopes: ["org:acme"],
      limitsPollMinutes: 10,
      resetAlerts: { "acct-work": alert },
    });
    await waitFor(() => expect(limitsProps.current?.resetAlerts).toEqual({ "acct-work": alert }));
  });

  it("fails the change when the settings could not be saved", async () => {
    api.saveAppSettings.mockRejectedValue({ kind: "message", message: "disk full", path: null });

    await expect((await onResetAlertsChange())({ "acct-work": alert })).rejects.toThrow(
      "The banked reset alert was not saved.",
    );
  });
});
