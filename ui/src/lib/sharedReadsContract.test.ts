import { describe, expect, expectTypeOf, it, vi } from "vitest";
import rustSharedReads from "../../../src-tauri/src/read_revision/fixtures/shared-reads.json";
import { onSharedReadChanged } from "./api";
import type { SharedReadChanged, SharedReadSource } from "./types";

const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen }));

type Announced = typeof rustSharedReads.announcements;

describe("the announcements Rust sends", () => {
  it("name exactly the sources the UI's type names, in the payload shape it reads", () => {
    expectTypeOf<SharedReadSource>().toEqualTypeOf<keyof Announced>();
    expectTypeOf<keyof SharedReadChanged>().toEqualTypeOf<keyof Announced[keyof Announced]>();
  });

  it("arrive under the event the UI listens to, each payload handed on as it came", async () => {
    let deliver: ((event: { payload: unknown }) => void) | undefined;
    listen.mockImplementation(async (_event: string, handler: (event: { payload: unknown }) => void) => {
      deliver = handler;
      return () => undefined;
    });
    const changes: SharedReadChanged[] = [];

    await onSharedReadChanged((change) => changes.push(change));
    for (const payload of Object.values(rustSharedReads.announcements)) deliver?.({ payload });

    expect(listen).toHaveBeenCalledWith(rustSharedReads.event, expect.any(Function));
    expect(changes).toEqual(Object.values(rustSharedReads.announcements));
  });
});
