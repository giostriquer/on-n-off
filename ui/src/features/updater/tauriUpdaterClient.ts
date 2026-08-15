import * as api from "$lib/api";
import type { UpdateResource, UpdaterClient } from "./updaterClient";

export const tauriUpdaterClient: UpdaterClient = {
  buildInfo: () => api.updaterBuildInfo(),
  currentVersion: async () => {
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  },
  check: async (target) => {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check({ target });
    if (!update) {
      return null;
    }
    const resource: UpdateResource = {
      version: update.version,
      date: update.date ?? null,
      body: update.body ?? null,
      download: (onEvent) => update.download(onEvent),
      install: () => update.install(),
      close: () => update.close(),
    };
    return resource;
  },
  relaunch: async () => {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  },
};
