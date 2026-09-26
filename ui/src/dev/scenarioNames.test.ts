import { describe, expect, it } from "vitest";
import { unknownScenario } from "./scenarioNames";

describe("unknownScenario", () => {
  it.each([
    // Pull-request and Limits scenarios, one of each table.
    "ok", "stale", "limitsBand", "accountDuplicate",
    // The ones mockIpc.ts answers for itself.
    "accountLogin", "accountLocked", "accountClients", "catalog", "hooks", "mcpSources",
  ])("knows %s", name => {
    expect(unknownScenario(name)).toBeNull();
  });

  it("names a mistyped scenario and lists the ones it could have meant", () => {
    const problem = unknownScenario("limitsBnad");
    expect(problem).toMatch(/^\[mock\] unknown scenario "limitsBnad"; known: /);
    expect(problem).toContain("limitsBand");
    expect(problem).toContain("hooks");
  });
});
