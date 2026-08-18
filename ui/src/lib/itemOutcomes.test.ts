import { describe, expect, it } from "vitest";
import { installOutcomeClean, summarizeOutcomes } from "./itemOutcomes";
import type { InstallItemsResult, ItemOutcome } from "./types";

function outcome(overrides: Partial<ItemOutcome>): ItemOutcome {
  return {
    provider: "claude",
    kind: "skill",
    name: "tdd",
    pluginName: "p",
    path: "skills/tdd",
    targetPath: "x",
    status: "installed",
    reason: null,
    ...overrides,
  };
}

describe("itemOutcomes", () => {
  it("summarises outcomes and knows when the sheet can close", () => {
    const result: InstallItemsResult = {
      commitSha: "b".repeat(40),
      shaMoved: false,
      outcomes: [
        outcome({}),
        outcome({ provider: "codex", status: "replaced" }),
        outcome({ provider: "codex", kind: "agent", name: "reviewer", status: "skipped", reason: "n/a" }),
        outcome({ name: "grilling", status: "conflict", reason: "exists" }),
        outcome({ provider: "cursor", name: "wizard", status: "failed", reason: "disk full" }),
      ],
    };
    const summary = summarizeOutcomes(result);
    expect(summary.installed).toBe(2);
    expect(summary.skipped).toBe(1);
    expect(summary.conflicts.map((o) => o.name)).toEqual(["grilling"]);
    expect(summary.failed.map((o) => o.name)).toEqual(["wizard"]);
    expect(summary.touchedProviders).toEqual(["claude", "codex"]);
    expect(installOutcomeClean(result)).toBe(false);
    expect(installOutcomeClean({ ...result, outcomes: result.outcomes.slice(0, 3) })).toBe(true);
    expect(installOutcomeClean({ ...result, outcomes: [outcome({ status: "failed" })] })).toBe(false);
  });
});
