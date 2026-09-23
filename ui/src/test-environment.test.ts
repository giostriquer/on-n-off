import { describe, expect, it } from "vitest";

// vitest.config.ts pins `test.env.TZ` to UTC. Tests fake fixed instants (Usage.test.tsx sets
// 2026-08-15T15:00Z) while the code under test reads the host's zone, so without the pin the same
// instant falls on another calendar day at UTC+9 and east, and the suite fails on those machines
// only. The pin can be lost two ways, and each test below catches one. A test that checks the zone
// cannot fail on a host that is already in UTC, so CI runs the suite at TZ=Asia/Tokyo (the "Test
// frontend" step in .github/workflows/ci.yml).
describe("test environment", () => {
  // The pin deleted: the worker inherits the host's TZ, or none at all. `import.meta.env` is
  // vitest's view of the worker's `process.env`, which ui/src has no Node types to name.
  it("hands every worker TZ=UTC", () => {
    expect(import.meta.env.TZ).toBe("UTC");
  });

  // A pool that cannot honour the pin. It reaches `Date` and `Intl` because every worker is its own
  // process; under the `threads` and `vmThreads` pools Node ignores a TZ set this way.
  it("runs Date and Intl in UTC whatever the host's zone", () => {
    expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("UTC");
    expect(new Date("2026-08-15T15:00:00.000Z").getTimezoneOffset()).toBe(0);
  });
});
