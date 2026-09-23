import { describe, expect, it } from "vitest";

// vitest.config.ts pins `test.env.TZ` to UTC. Tests fake fixed instants (Usage.test.tsx sets
// 2026-08-15T15:00Z) while the code under test reads the host's zone, so without the pin the same
// instant falls on another calendar day at UTC+9 and east, and the suite fails on those machines
// only. The pin reaches `Date` and `Intl` because every worker is its own process; under the
// `threads` and `vmThreads` pools Node ignores a TZ set this way, and this test is what notices.
describe("test environment", () => {
  it("runs every worker in UTC whatever the host's zone", () => {
    expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("UTC");
    expect(new Date("2026-08-15T15:00:00.000Z").getTimezoneOffset()).toBe(0);
  });
});
