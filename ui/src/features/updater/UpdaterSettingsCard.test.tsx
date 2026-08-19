import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { UpdateProvider } from "./UpdateProvider";
import { UpdaterSettingsCard } from "./UpdaterSettingsCard";
import type { UpdaterClient } from "./updaterClient";

describe("UpdaterSettingsCard", () => {
  it("shows the installed version, persists the toggle, and supports a manual check", async () => {
    const user = userEvent.setup();
    let checks = 0;
    let requestedAutomaticValue: boolean | null = null;
    const client: UpdaterClient = {
      buildInfo: async () => ({
        enabled: true,
        installerKind: "nsis",
        target: "windows-x86_64-nsis",
      }),
      currentVersion: async () => "0.1.0",
      check: async () => {
        checks += 1;
        return null;
      },
      relaunch: async () => undefined,
    };
    render(
      <UpdateProvider initialProviderReady automaticUpdates={false} client={client}>
        <UpdaterSettingsCard
          automaticUpdates={false}
          onAutomaticUpdatesChange={(enabled) => {
            requestedAutomaticValue = enabled;
          }}
        />
      </UpdateProvider>,
    );

    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByText("Stable")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Automatically download updates" }));
    expect(requestedAutomaticValue).toBe(true);

    await user.click(screen.getByRole("button", { name: "Check now" }));
    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(checks).toBe(1);
  });

  it("offers a retry after a check failure", async () => {
    const user = userEvent.setup();
    let checks = 0;
    const client: UpdaterClient = {
      buildInfo: async () => ({
        enabled: true,
        installerKind: "nsis",
        target: "windows-x86_64-nsis",
      }),
      currentVersion: async () => "0.1.0",
      check: async () => {
        checks += 1;
        if (checks === 1) {
          throw new Error("feed unavailable");
        }
        return null;
      },
      relaunch: async () => undefined,
    };
    render(
      <UpdateProvider initialProviderReady automaticUpdates={false} client={client}>
        <UpdaterSettingsCard automaticUpdates={false} onAutomaticUpdatesChange={() => undefined} />
      </UpdateProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Check now" }));
    expect(await screen.findByText("feed unavailable")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry update check" }));

    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(checks).toBe(2);
  });

  it("offers a retry when native updater initialization fails", async () => {
    const user = userEvent.setup();
    let buildInfoCalls = 0;
    const client: UpdaterClient = {
      buildInfo: async () => {
        buildInfoCalls += 1;
        if (buildInfoCalls === 1) {
          throw new Error("update configuration unavailable");
        }
        return {
          enabled: true,
          installerKind: "nsis",
          target: "windows-x86_64-nsis",
        };
      },
      currentVersion: async () => "0.1.0",
      check: async () => null,
      relaunch: async () => undefined,
    };
    render(
      <UpdateProvider initialProviderReady automaticUpdates={false} client={client}>
        <UpdaterSettingsCard automaticUpdates={false} onAutomaticUpdatesChange={() => undefined} />
      </UpdateProvider>,
    );

    expect(await screen.findByText("update configuration unavailable")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry update check" }));

    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(buildInfoCalls).toBe(2);
  });
});
