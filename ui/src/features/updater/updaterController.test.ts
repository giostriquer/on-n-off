import { beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateController } from "./updaterController";
import type { UpdaterClient, UpdateResource } from "./updaterClient";

function resource(overrides: Partial<UpdateResource> = {}): UpdateResource {
  return {
    version: "0.2.0",
    date: "2026-08-15T12:00:00Z",
    body: "A safer release.",
    download: async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Finished", data: {} });
    },
    install: async () => undefined,
    close: async () => undefined,
    ...overrides,
  };
}

function client(overrides: Partial<UpdaterClient> = {}): UpdaterClient {
  return {
    buildInfo: async () => ({
      enabled: true,
      installerKind: "nsis",
      target: "windows-x86_64-nsis",
    }),
    currentVersion: async () => "0.1.0",
    check: async () => resource(),
    relaunch: async () => undefined,
    ...overrides,
  };
}

describe("UpdateController", () => {
  beforeEach(() => {
    Object.defineProperty(performance, "mark", { configurable: true, value: vi.fn() });
  });

  it("waits for a manual check when automatic updates are disabled", async () => {
    let checks = 0;
    const controller = new UpdateController(
      client({
        check: async () => {
          checks += 1;
          return resource();
        },
      }),
    );

    await controller.initialize(false);
    expect(controller.getSnapshot()).toMatchObject({
      currentVersion: "0.1.0",
      buildInfo: { enabled: true, installerKind: "nsis" },
      state: { status: "idle" },
    });
    expect(checks).toBe(0);

    await controller.checkNow();
    expect(checks).toBe(1);
    expect(controller.getSnapshot().state).toEqual({
      status: "ready",
      update: {
        version: "0.2.0",
        date: "2026-08-15T12:00:00Z",
        body: "A safer release.",
      },
      dismissed: false,
    });
  });

  it("checks and downloads automatically when enabled", async () => {
    const controller = new UpdateController(client());

    await controller.initialize(true);

    expect(controller.getSnapshot().state.status).toBe("ready");
    expect(performance.mark).toHaveBeenCalledWith("on-n-off:updater-check-start");
  });

  it("deduplicates concurrent checks", async () => {
    let checks = 0;
    let releaseCheck!: (value: UpdateResource | null) => void;
    const pending = new Promise<UpdateResource | null>((resolve) => {
      releaseCheck = resolve;
    });
    const controller = new UpdateController(
      client({
        check: () => {
          checks += 1;
          return pending;
        },
      }),
    );
    await controller.initialize(false);

    const first = controller.checkNow();
    const second = controller.checkNow();
    expect(checks).toBe(1);
    releaseCheck(null);
    await Promise.all([first, second]);

    expect(controller.getSnapshot().state.status).toBe("upToDate");
  });

  it("discards a failed install and requires a fresh check", async () => {
    let closes = 0;
    const controller = new UpdateController(
      client({
        check: async () =>
          resource({
            install: async () => {
              throw new Error("installer refused");
            },
            close: async () => {
              closes += 1;
            },
          }),
      }),
    );
    await controller.initialize(true);

    await controller.install();

    expect(closes).toBe(1);
    expect(controller.getSnapshot().state).toMatchObject({
      status: "error",
      operation: "install",
      message: "installer refused",
    });
  });

  it("installs and relaunches a downloaded update", async () => {
    let installs = 0;
    let relaunches = 0;
    const controller = new UpdateController(
      client({
        check: async () =>
          resource({
            install: async () => {
              installs += 1;
            },
          }),
        relaunch: async () => {
          relaunches += 1;
        },
      }),
    );

    await controller.initialize(true);
    await controller.install();

    expect(installs).toBe(1);
    expect(relaunches).toBe(1);
  });

  it("closes a downloaded native update during cleanup", async () => {
    let closes = 0;
    const controller = new UpdateController(
      client({
        check: async () =>
          resource({
            close: async () => {
              closes += 1;
            },
          }),
      }),
    );

    await controller.initialize(true);
    await controller.dispose();

    expect(closes).toBe(1);
  });
});
