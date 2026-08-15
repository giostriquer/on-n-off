import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import * as api from "$lib/api";
import type { UpdateResource, UpdaterClient } from "./updaterClient";

export const tauriUpdaterClient: UpdaterClient = {
  buildInfo: () => api.updaterBuildInfo(),
  currentVersion: () => getVersion(),
  check: async (target) => {
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
  relaunch: () => relaunch(),
};
