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

  it("archives archivedAccounts' saved reading, profile-only login and history, and follows archive and unarchive for that page alone", () => {
    const scenario = limitsScenario("archivedAccounts");
    const archived = (agent: string) => [
      ...scenario.readLimits(agent).filter(entry => entry.archived).map(entry => entry.account?.id),
      ...scenario.readAccounts(agent).profiles.filter(profile => profile.archived).map(profile => profile.observationId),
    ];
    expect(archived("claude")).toEqual(["claude-2", "claude-2", "profile:claude-unread"]);
    expect(scenario.readAccounts("claude").profiles.map(profile => profile.observationId)).toContain("profile:claude-unread");
    expect(ids(scenario.readLimits("claude"))).not.toContain("profile:claude-unread");
    expect(archived("codex")).toEqual(["codex-history"]);
    expect(scenario.readLimits("codex").find(entry => entry.account?.id === "codex-history")?.savedProfile).toBeFalsy();

    scenario.setArchived("codex", ["codex-2"], true);
    scenario.setArchived("claude", ["claude-2"], false);
    expect(archived("codex")).toEqual(["codex-2", "codex-history", "codex-2"]);
    expect(archived("claude")).toEqual(["profile:claude-unread"]);
    expect(scenario.readLimits("claude")[0].archived, "never the signed-in card").toBeFalsy();
    // Another page's scenario starts clean, and one without archive state ignores it.
    expect(limitsScenario("archivedAccounts").readLimits("codex").filter(entry => entry.archived).map(entry => entry.account?.id)).toEqual(["codex-history"]);
    limitsScenario("ok").setArchived("codex", ["codex-2"], true);
    expect(limitsScenario("ok").readLimits("codex").some(entry => entry.archived)).toBe(false);
  });

  it("drops what Remove account removes, a snapshot and a saved login, for that page alone", () => {
    const scenario = limitsScenario("ok");
    const profiles = (agent: string) => scenario.readAccounts(agent).profiles.map(profile => profile.id);
    scenario.removeLogin("codex", "work");
    scenario.forgetSnapshot("codex", "codex-2");
    expect(ids(scenario.readLimits("codex"))).toEqual(["codex-1"]);
    expect(profiles("codex")).toEqual(["personal"]);
    expect(ids(scenario.readLimits("claude")), "only the provider named").toEqual(["claude-1"]);
    expect(profiles("claude")).toEqual(["personal", "work"]);

    scenario.forgetSnapshot("codex", "codex-1");
    expect(ids(scenario.readLimits("codex")), "the signed-in card is a live read, not a snapshot").toEqual(["codex-1"]);
    // Another page's scenario starts clean.
    expect(ids(limitsScenario("ok").readLimits("codex"))).toEqual(["codex-1", "codex-2"]);
    expect(limitsScenario("ok").readAccounts("codex").profiles.map(profile => profile.id)).toEqual(["personal", "work"]);
  });

  it("unarchives what archivedAccounts forgets, as the backend does", () => {
    const scenario = limitsScenario("archivedAccounts");
    scenario.forgetSnapshot("codex", "codex-history");
    expect(ids(scenario.readLimits("codex"))).toEqual(["codex-1", "codex-2"]);

    // Remove account removes a saved login before it forgets; forgetting alone leaves the login, unarchived.
    scenario.forgetSnapshot("claude", "profile:claude-unread");
    const profiles = scenario.readAccounts("claude").profiles;
    expect(profiles.map(profile => profile.id)).toEqual(["personal", "work", "unread"]);
    expect(profiles[2].archived).toBeFalsy();
    scenario.removeLogin("claude", "unread");
    expect(scenario.readAccounts("claude").profiles.map(profile => profile.id)).toEqual(["personal", "work"]);
  });
});
