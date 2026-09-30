import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_APP_SETTINGS } from "$lib/appSettings";
import type { AppSettings, ResetAlert } from "$lib/types";
import { LimitsRoute } from "./limits";

// The route hands Limits the app's banked reset alerts and saves a change through the session,
// whose failed save must reach the alert's form as one.

const persistAppSettings = vi.hoisted(() => vi.fn<(next: AppSettings) => Promise<AppSettings | null>>());
const limitsProps = vi.hoisted(() => ({ current: null as null | {
  resetAlerts: Record<string, ResetAlert>;
  onResetAlertsChange: (alerts: Record<string, ResetAlert>) => Promise<void>;
} }));

vi.mock("@/features/session/SessionProvider", async () => {
  const { DEFAULT_APP_SETTINGS } = await import("$lib/appSettings");
  return { useAgentSession: () => ({ appSettings: { ...DEFAULT_APP_SETTINGS, githubScopes: ["org:acme"] }, persistAppSettings }) };
});

vi.mock("@/features/limits/Limits", () => ({
  Limits: (props: NonNullable<typeof limitsProps.current>) => {
    limitsProps.current = props;
    return null;
  },
}));

const alert: ResetAlert = { label: "you@example.com", maxLeftPercent: 5, minHoursToRenewal: 48 };

function onResetAlertsChange() {
  render(<LimitsRoute />);
  return limitsProps.current!.onResetAlertsChange;
}

beforeEach(() => {
  persistAppSettings.mockReset();
  limitsProps.current = null;
});

describe("the Limits route's banked reset alerts", () => {
  it("saves a change with the rest of the app's settings", async () => {
    persistAppSettings.mockImplementation(async (next) => next);

    await onResetAlertsChange()({ "acct-work": alert });

    expect(persistAppSettings).toHaveBeenCalledWith({
      ...DEFAULT_APP_SETTINGS,
      githubScopes: ["org:acme"],
      resetAlerts: { "acct-work": alert },
    });
  });

  it("fails the change when the session could not save it", async () => {
    persistAppSettings.mockResolvedValue(null);

    await expect(onResetAlertsChange()({ "acct-work": alert })).rejects.toThrow();
  });
});
