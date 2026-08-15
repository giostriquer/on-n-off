import { describe, expect, it } from "vitest";
import { INITIAL_UPDATE_STATE, updateReducer, type UpdateMetadata } from "./updaterState";

const update: UpdateMetadata = {
  version: "0.2.0",
  date: "2026-08-15T12:00:00Z",
  body: "A safer release.",
};

describe("updateReducer", () => {
  it("tracks a downloaded update through progress to ready", () => {
    const checking = updateReducer(INITIAL_UPDATE_STATE, { type: "CHECK_STARTED" });
    const started = updateReducer(checking, {
      type: "DOWNLOAD_STARTED",
      update,
      contentLength: 100,
    });
    const progressing = updateReducer(started, { type: "DOWNLOAD_PROGRESS", chunkLength: 40 });
    const ready = updateReducer(progressing, { type: "DOWNLOAD_FINISHED" });

    expect(checking).toEqual({ status: "checking" });
    expect(progressing).toEqual({
      status: "downloading",
      update,
      downloaded: 40,
      contentLength: 100,
    });
    expect(ready).toEqual({ status: "ready", update, dismissed: false });
  });

  it("caps progress at the known content length", () => {
    const started = updateReducer(INITIAL_UPDATE_STATE, {
      type: "DOWNLOAD_STARTED",
      update,
      contentLength: 100,
    });

    expect(updateReducer(started, { type: "DOWNLOAD_PROGRESS", chunkLength: 140 })).toMatchObject({
      downloaded: 100,
    });
  });

  it("dismisses only the global prompt and keeps the ready update", () => {
    const ready = { status: "ready", update, dismissed: false } as const;

    expect(updateReducer(ready, { type: "DISMISS" })).toEqual({ status: "ready", update, dismissed: true });
  });

  it("records retryable operation failures with available metadata", () => {
    expect(
      updateReducer(INITIAL_UPDATE_STATE, {
        type: "FAILED",
        operation: "download",
        message: "connection reset",
        update,
      }),
    ).toEqual({
      status: "error",
      operation: "download",
      message: "connection reset",
      update,
    });
  });
});
