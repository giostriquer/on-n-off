import type { UpdaterBuildInfo } from "$lib/types";
import type { UpdateResource, UpdaterClient } from "./updaterClient";
import {
  INITIAL_UPDATE_STATE,
  updateReducer,
  type UpdateEvent,
  type UpdateMetadata,
  type UpdateState,
} from "./updaterState";

export type UpdaterSnapshot = {
  state: UpdateState;
  buildInfo: UpdaterBuildInfo | null;
  currentVersion: string | null;
};

export class UpdateController {
  private snapshot: UpdaterSnapshot = {
    state: INITIAL_UPDATE_STATE,
    buildInfo: null,
    currentVersion: null,
  };
  private readonly listeners = new Set<() => void>();
  private initializeInFlight: Promise<void> | null = null;
  private checkInFlight: Promise<void> | null = null;
  private updateResource: UpdateResource | null = null;

  constructor(private readonly client: UpdaterClient) {}

  getSnapshot = (): UpdaterSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private publish(patch: Partial<UpdaterSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    this.listeners.forEach((listener) => listener());
  }

  private dispatch(event: UpdateEvent): void {
    this.publish({ state: updateReducer(this.snapshot.state, event) });
  }

  async initialize(automaticUpdates: boolean): Promise<void> {
    if (this.snapshot.buildInfo) {
      if (automaticUpdates) {
        await this.checkNow();
      }
      return;
    }
    if (this.initializeInFlight) {
      await this.initializeInFlight;
      return;
    }

    const initialize = async () => {
      try {
        const [buildInfo, currentVersion] = await Promise.all([
          this.client.buildInfo(),
          this.client.currentVersion(),
        ]);
        this.publish({ buildInfo, currentVersion });
        if (automaticUpdates && buildInfo.enabled) {
          await this.checkNow();
        }
      } catch (error) {
        this.dispatch({ type: "FAILED", operation: "check", message: errorMessage(error) });
      }
    };
    this.initializeInFlight = initialize();
    try {
      await this.initializeInFlight;
    } finally {
      this.initializeInFlight = null;
    }
  }

  async checkNow(): Promise<void> {
    if (this.checkInFlight) {
      await this.checkInFlight;
      return;
    }
    if (!this.snapshot.buildInfo) {
      await this.initialize(false);
    }
    const { buildInfo } = this.snapshot;
    if (!buildInfo?.enabled || !buildInfo.target || this.updateResource) {
      return;
    }

    const check = this.performCheck(buildInfo.target);
    this.checkInFlight = check;
    try {
      await check;
    } finally {
      this.checkInFlight = null;
    }
  }

  private async performCheck(target: string): Promise<void> {
    this.dispatch({ type: "CHECK_STARTED" });
    let update: UpdateResource | null = null;
    try {
      update = await this.client.check(target);
      if (!update) {
        this.dispatch({ type: "NO_UPDATE", checkedAt: new Date().toISOString() });
        return;
      }
      const metadata: UpdateMetadata = {
        version: update.version,
        date: update.date,
        body: update.body,
      };
      this.updateResource = update;
      this.dispatch({ type: "DOWNLOAD_STARTED", update: metadata, contentLength: null });
      await update.download((event) => {
        if (event.event === "Started") {
          this.dispatch({
            type: "DOWNLOAD_STARTED",
            update: metadata,
            contentLength: event.data.contentLength ?? null,
          });
        } else if (event.event === "Progress") {
          this.dispatch({ type: "DOWNLOAD_PROGRESS", chunkLength: event.data.chunkLength });
        }
      });
      this.dispatch({ type: "DOWNLOAD_FINISHED" });
    } catch (error) {
      const operation = update ? "download" : "check";
      const metadata = update
        ? { version: update.version, date: update.date, body: update.body }
        : null;
      await this.closeUpdateResource();
      this.dispatch({ type: "FAILED", operation, message: errorMessage(error), update: metadata });
    }
  }

  dismiss(): void {
    this.dispatch({ type: "DISMISS" });
  }

  async install(): Promise<void> {
    if (this.snapshot.state.status !== "ready" || !this.updateResource) {
      return;
    }
    const metadata = this.snapshot.state.update;
    this.dispatch({ type: "INSTALL_STARTED" });
    try {
      await this.updateResource.install();
      await this.client.relaunch();
    } catch (error) {
      await this.closeUpdateResource();
      this.dispatch({
        type: "FAILED",
        operation: "install",
        message: errorMessage(error),
        update: metadata,
      });
    }
  }

  private async closeUpdateResource(): Promise<void> {
    const update = this.updateResource;
    this.updateResource = null;
    if (!update) {
      return;
    }
    try {
      await update.close();
    } catch {
      // The process can already be exiting or the native resource can be gone.
    }
  }

  async dispose(): Promise<void> {
    await this.closeUpdateResource();
    this.listeners.clear();
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
