import { describe, expect, it } from "vitest";

describe("test environment", () => {
  it("hands every worker TZ=UTC", () => {
    expect(import.meta.env.TZ).toBe("UTC");
  });

  it("runs Date and Intl in UTC whatever the host's zone", () => {
    expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("UTC");
    expect(new Date("2026-08-15T15:00:00.000Z").getTimezoneOffset()).toBe(0);
  });
});
