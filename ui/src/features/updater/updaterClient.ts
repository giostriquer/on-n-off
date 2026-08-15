import type { UpdaterBuildInfo } from "$lib/types";
import type { UpdateMetadata } from "./updaterState";

export type UpdateDownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished"; data?: Record<string, never> };

export type UpdateResource = UpdateMetadata & {
  download: (onEvent: (event: UpdateDownloadEvent) => void) => Promise<void>;
  install: () => Promise<void>;
  close: () => Promise<void>;
};

export type UpdaterClient = {
  buildInfo: () => Promise<UpdaterBuildInfo>;
  currentVersion: () => Promise<string>;
  check: (target: string) => Promise<UpdateResource | null>;
  relaunch: () => Promise<void>;
};
