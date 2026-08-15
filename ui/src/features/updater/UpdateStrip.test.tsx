import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { UpdateProvider } from "./UpdateProvider";
import { UpdateStrip } from "./UpdateStrip";
import type { UpdaterClient, UpdateResource } from "./updaterClient";

function client(update: UpdateResource): UpdaterClient {
  return {
    buildInfo: async () => ({
      enabled: true,
      installerKind: "nsis",
      target: "windows-x86_64-nsis",
    }),
    currentVersion: async () => "0.1.0",
    check: async () => update,
    relaunch: async () => undefined,
  };
}

function updateResource(install: () => Promise<void> = async () => undefined): UpdateResource {
  return {
    version: "0.2.0",
    date: "2026-08-15T12:00:00Z",
    body: "A safer release.",
    download: async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 10 } });
      onEvent({ event: "Progress", data: { chunkLength: 10 } });
    },
    install,
    close: async () => undefined,
  };
}

describe("UpdateStrip", () => {
  it("offers a downloaded update and dismisses only the strip for Later", async () => {
    const user = userEvent.setup();
    render(
      <UpdateProvider initialProviderReady automaticUpdates client={client(updateResource())}>
        <UpdateStrip />
      </UpdateProvider>,
    );

    expect(await screen.findByRole("region", { name: "Update available" })).toHaveTextContent("0.2.0");
    await user.click(screen.getByRole("button", { name: "Later" }));

    expect(screen.queryByRole("region", { name: "Update available" })).toBeNull();
  });

  it("starts installation only after the user selects Install and restart", async () => {
    const user = userEvent.setup();
    let installs = 0;
    render(
      <UpdateProvider
        initialProviderReady
        automaticUpdates
        client={client(
          updateResource(async () => {
            installs += 1;
          }),
        )}
      >
        <UpdateStrip />
      </UpdateProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Install and restart" }));

    expect(installs).toBe(1);
  });
});
