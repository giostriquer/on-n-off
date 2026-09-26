import { afterEach, describe, expect, it, vi } from "vitest";
import { limitsScenario } from "./limitsFixtures";

const ids = (entries: { account?: { id: string } | null }[]) => entries.map(entry => entry.account?.id);

afterEach(() => {
  vi.useRealTimers();
});

describe("limitsScenario", () => {
  it("answers a scenario's own readings, and ok's for the provider it leaves out", () => {
    const scenario = limitsScenario("claudeNoWeekly");
    const claude = scenario.readLimits("claude");
    expect(ids(claude)).toEqual(["claude-1", "claude-2"]);
    expect(claude[1].windows.map(window => window.kind)).toEqual(["session", "model"]);
    expect(ids(scenario.readLimits("codex"))).toEqual(["codex-1", "codex-2"]);
    expect(scenario.readLimits("antigravity")).toEqual([]);
  });

  it("answers a scenario's saved accounts for one provider, and the usual ones for the other", () => {
    const scenario = limitsScenario("sameEmailWorkspaces");
    expect(scenario.readAccounts("codex")).toMatchObject({ nativeObservationId: "profile:personal" });
    expect(scenario.readAccounts("codex").profiles.map(profile => profile.observationId)).toEqual(["profile:personal", "profile:business"]);
    expect(scenario.readAccounts("claude").profiles.map(profile => profile.observationId)).toEqual(["claude-1", "claude-2"]);
  });

  it("dates a paid-through answer from the clock, unless the scenario has none", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-08-24T20:00:00Z"));
    expect(limitsScenario("ok").readCodexSubscription("codex-2")).toEqual({ date: "2026-08-27T20:00:00.000Z", checkedAt: "2026-08-16T20:00:00.000Z" });
    expect(limitsScenario("subscriptionMissing").readCodexSubscription("codex-2")).toBeNull();
  });

  it("answers a name that is not a Limits scenario as ok does", () => {
    expect(limitsScenario("limitsBnad").readLimits("claude")).toEqual(limitsScenario("ok").readLimits("claude"));
    expect(limitsScenario("stale").readLimits("codex")).toEqual(limitsScenario("ok").readLimits("codex"));
  });

  it("gives accountDuplicate its saved Codex login and the usual Claude ones", () => {
    const scenario = limitsScenario("accountDuplicate");
    expect(scenario.readAccounts("codex")).toMatchObject({ nativeObservationId: "codex-1" });
    expect(scenario.readAccounts("codex").profiles.map(profile => profile.observationId)).toEqual(["profile:shared"]);
    expect(scenario.readAccounts("claude").profiles.map(profile => profile.observationId)).toEqual(["claude-1", "claude-2"]);
  });

  it("brings accountDuplicate's scoped reading only once a sign-in starts, for that page alone", () => {
    const scenario = limitsScenario("accountDuplicate");
    const legacy = "ca292064-c3f4-453c-b15a-43ef63c46478";
    expect(ids(scenario.readLimits("codex"))).toEqual(["codex-1", legacy]);
    limitsScenario("ok").addAccount();
    expect(ids(scenario.readLimits("codex"))).toEqual(["codex-1", legacy]);
    scenario.addAccount();
    expect(ids(scenario.readLimits("codex"))).toEqual(["codex-1", "profile:shared", legacy]);
    // Another page's scenario starts clean.
    expect(ids(limitsScenario("accountDuplicate").readLimits("codex"))).toEqual(["codex-1", legacy]);
  });
});
