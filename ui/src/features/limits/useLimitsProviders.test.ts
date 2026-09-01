import { describe, expect, it } from "vitest";
import { limitsRefreshMs } from "./useLimitsProviders";

describe("limits refresh policy", () => {
  it("converts every supported setting to one refresh interval", () => {
    const fiveMinutes = limitsRefreshMs(5);
    expect(fiveMinutes).toBe(300_000);
    expect(limitsRefreshMs(10)).toBe(600_000);
  });
});
