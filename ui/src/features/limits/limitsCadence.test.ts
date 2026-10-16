import { describe, expect, it } from "vitest";
import cadence from "../../../../src-tauri/src/limits_refresh/fixtures/cadence.json";
import type { LimitsPollMinutes } from "$lib/types";
import { LIMITS_CHECK_MINUTES, limitsRefreshMs } from "./useLimitsProviders";

describe("the Limits check interval Rust keeps", () => {
  it("checks the shared cache at least as often as the Rust consumers do", () => {
    expect(LIMITS_CHECK_MINUTES * 60).toBe(cadence.checkSeconds);
    for (const minutes of [5, 10, 15, 30] satisfies LimitsPollMinutes[]) {
      expect(limitsRefreshMs(minutes)).toBe(Math.min(minutes * 60, cadence.checkSeconds) * 1000);
    }
  });
});
