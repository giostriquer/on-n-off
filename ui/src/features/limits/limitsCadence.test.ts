import { describe, expect, it } from "vitest";
import cadence from "../../../../src-tauri/src/limits_refresh/fixtures/cadence.json";
import type { LimitsPollMinutes } from "$lib/types";
import { LIMITS_CHECK_MINUTES, limitsQuery } from "$lib/sharedReads/limits";

describe("the Limits check interval Rust keeps", () => {
  it("checks the shared cache at least as often as the Rust consumers do", () => {
    expect(LIMITS_CHECK_MINUTES * 60).toBe(cadence.checkSeconds);
    for (const minutes of [5, 10, 15, 30] satisfies LimitsPollMinutes[]) {
      const query = limitsQuery("claude", minutes);
      expect(query.refetchInterval).toBe(Math.min(minutes * 60, cadence.checkSeconds) * 1000);
      expect(query.staleTime).toBe(Math.min(minutes * 60, cadence.checkSeconds) * 1000);
    }
  });
});
