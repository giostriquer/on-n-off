import { beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  imports: [] as string[],
  getVersion: vi.fn(async () => "0.1.1"),
  check: vi.fn(async (options: { target: string }) => ({
    version: "0.2.0",
    date: "2026-08-15T12:00:00Z",
    body: "A safer release.",
    download: native.download,
    install: native.install,
    close: native.close,
    options,
  })),
  download: vi.fn(async () => undefined),
  install: vi.fn(async () => undefined),
  close: vi.fn(async () => undefined),
  relaunch: vi.fn(async () => undefined),
}));

vi.mock("$lib/api", () => ({
  updaterBuildInfo: vi.fn(async () => ({
    enabled: true,
    installerKind: "nsis",
    target: "windows-x86_64-nsis",
  })),
}));

vi.mock("@tauri-apps/api/app", () => {
  native.imports.push("app");
  return { getVersion: native.getVersion };
});

vi.mock("@tauri-apps/plugin-updater", () => {
  native.imports.push("updater");
  return { check: native.check };
});

vi.mock("@tauri-apps/plugin-process", () => {
  native.imports.push("process");
  return { relaunch: native.relaunch };
});

describe("tauriUpdaterClient bundle boundaries", () => {
  beforeEach(() => {
    native.imports.length = 0;
    native.getVersion.mockClear();
    native.check.mockClear();
    native.download.mockClear();
    native.install.mockClear();
    native.close.mockClear();
    native.relaunch.mockClear();
    vi.resetModules();
  });

  it("loads each native integration only when its updater method needs it", async () => {
    const { tauriUpdaterClient } = await import("./tauriUpdaterClient");

    expect(native.imports).toEqual([]);
    await expect(tauriUpdaterClient.buildInfo()).resolves.toMatchObject({ enabled: true });
    expect(native.imports).toEqual([]);

    await expect(tauriUpdaterClient.currentVersion()).resolves.toBe("0.1.1");
    expect(native.imports).toEqual(["app"]);

    const update = await tauriUpdaterClient.check("windows-x86_64-nsis");
    expect(native.imports).toEqual(["app", "updater"]);
    expect(native.check).toHaveBeenCalledWith({ target: "windows-x86_64-nsis" });
    expect(update).toMatchObject({
      version: "0.2.0",
      date: "2026-08-15T12:00:00Z",
      body: "A safer release.",
    });

    const onEvent = vi.fn();
    await update?.download(onEvent);
    await update?.install();
    await update?.close();
    expect(native.download).toHaveBeenCalledWith(onEvent);
    expect(native.install).toHaveBeenCalledOnce();
    expect(native.close).toHaveBeenCalledOnce();

    await tauriUpdaterClient.relaunch();
    expect(native.imports).toEqual(["app", "updater", "process"]);
    expect(native.relaunch).toHaveBeenCalledOnce();
  });
});
