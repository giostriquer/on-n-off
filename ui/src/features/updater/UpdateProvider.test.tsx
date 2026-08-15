import { render, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { UpdateProvider } from "./UpdateProvider";
import type { UpdaterClient } from "./updaterClient";

describe("UpdateProvider", () => {
  it("does not initialize or check until the selected provider is ready", async () => {
    let buildInfoCalls = 0;
    let checks = 0;
    const client: UpdaterClient = {
      buildInfo: async () => {
        buildInfoCalls += 1;
        return { enabled: true, installerKind: "nsis", target: "windows-x86_64-nsis" };
      },
      currentVersion: async () => "0.1.0",
      check: async () => {
        checks += 1;
        return null;
      },
      relaunch: async () => undefined,
    };
    const view = render(
      <UpdateProvider initialProviderReady={false} automaticUpdates client={client}>
        <div>app</div>
      </UpdateProvider>,
    );

    expect(buildInfoCalls).toBe(0);
    expect(checks).toBe(0);

    view.rerender(
      <UpdateProvider initialProviderReady automaticUpdates client={client}>
        <div>app</div>
      </UpdateProvider>,
    );

    await waitFor(() => expect(checks).toBe(1));
    expect(buildInfoCalls).toBe(1);
  });
});
