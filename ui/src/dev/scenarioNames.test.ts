import { describe, expect, it } from "vitest";
import { unknownScenario } from "./scenarioNames";

describe("unknownScenario", () => {
  it.each(["ok", "stale", "limitsBand", "accountDuplicate", "hooks"])("knows %s", name => {
    expect(unknownScenario(name)).toBeNull();
  });

  it("names a mistyped scenario and lists the ones it could have meant", () => {
    const problem = unknownScenario("limitsBnad");
    expect(problem).toMatch(/^\[mock\] unknown scenario "limitsBnad"; known: /);
    expect(problem).toContain("limitsBand");
    expect(problem).toContain("hooks");
  });
});
