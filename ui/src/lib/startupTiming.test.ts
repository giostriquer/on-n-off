import { afterEach, describe, expect, it, vi } from "vitest";
import { markStartup } from "./startupTiming";

describe("markStartup", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("records namespaced performance marks when the runtime supports them", () => {
    const mark = vi.fn();
    Object.defineProperty(performance, "mark", { configurable: true, value: mark });

    markStartup("selected-local-ready");

    expect(mark).toHaveBeenCalledWith("on-n-off:selected-local-ready");
  });
});
