import { expect, it } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import { accountCards, orderAccountCards } from "./accountCards";

const profile: SavedProfile = {
  id: "saved", observationId: "profile:user-team", identity: { provider: "codex", userId: "user", workspaceId: "team" },
  email: "person@example.com", label: "person@example.com", savedAt: "2026-09-13T12:00:00Z", active: false, needsLogin: false,
};
const scoped: ProviderLimits = {
  provider: "codex", status: "ok", account: { id: profile.observationId!, label: profile.email }, currentAccount: false,
  windows: [{ id: "weekly", label: "Weekly", kind: "weekly", usedPercent: 0, observedAt: profile.savedAt }],
};
const legacy: ProviderLimits = { ...scoped, account: { id: "team", label: profile.email }, windows: [{ ...scoped.windows[0], usedPercent: 100 }] };

it("uses both verified workspace and email, preserving other users and workspaces", () => {
  const otherEmail = { ...legacy, account: { id: "team", label: "other@example.com" } };
  const otherWorkspace = { ...legacy, account: { id: "other-team", label: profile.email } };
  const otherProvider = { ...legacy, provider: "claude" as const };
  expect(accountCards([scoped, legacy, otherEmail, otherWorkspace, otherProvider], [profile]).entries)
    .toEqual([scoped, otherEmail, otherWorkspace, otherProvider]);
});
it("does not discard current legacy observations", () => {
  const current = { ...legacy, currentAccount: true };
  expect(accountCards([scoped, current], [profile]).entries).toEqual([scoped, current]);
});
it.each([
  { ...scoped, windows: [] },
  // The shape a current Codex read has when it reports no windows.
  { ...scoped, windows: [], resetCredits: { availableCount: 0, nextExpiresAt: null } },
  { ...scoped, status: "failed" as const },
  { ...scoped, account: { id: "profile:other-user", label: profile.email } },
  { ...scoped, account: { id: profile.observationId!, label: "other@example.com" } },
])("retains history when scoped usage cannot be verified: %j", candidate => {
  expect(accountCards([candidate, legacy], [profile]).entries).toEqual([candidate, legacy]);
});
it("counts a scoped read that carries only banked resets as an observation", () => {
  const resetsOnly = { ...scoped, windows: [], resetCredits: { availableCount: 1, nextExpiresAt: null } };
  expect(accountCards([resetsOnly, legacy], [profile]).entries).toEqual([resetsOnly]);
});
it("does not infer identity from email without a saved profile", () => {
  expect(accountCards([scoped, legacy], []).entries).toEqual([scoped, legacy]);
  expect(accountCards([legacy], [profile]).entries).toEqual([legacy]);
  expect(accountCards([scoped, legacy], [{ ...profile, email: " " }]).entries).toEqual([scoped, legacy]);
});
it("reconciles Claude using its user ID and normalizes display emails", () => {
  const claude = { ...scoped, provider: "claude" as const, account: { id: "profile:claude", label: " Person@Example.com " } };
  const old = { ...legacy, provider: "claude" as const, account: { id: "user", label: profile.email } };
  expect(accountCards([claude, old], [{ ...profile, observationId: "profile:claude", identity: { ...profile.identity, provider: "claude" } }]).entries).toEqual([claude]);
});

const NOW = Date.parse("2026-09-21T12:00:00Z");
const at = (hours: number) => new Date(NOW + hours * 3_600_000).toISOString();
let sequence = 0;
/** One account with a 5-hour and a weekly window; `usedPercent` and reset instants per window. */
const OBSERVED_AT = "2026-09-21T11:00:00Z";
function account(
  label: string,
  windows: { session?: [number, string | null]; weekly?: [number, string | null]; model?: [number, string | null] },
  overrides: Partial<ProviderLimits> = {},
): ProviderLimits {
  const window = (kind: "session" | "weekly" | "model", [usedPercent, resetsAt]: [number, string | null]) =>
    ({ id: kind, label: kind, kind, usedPercent, resetsAt, observedAt: OBSERVED_AT });
  return {
    provider: "claude", status: "ok", account: { id: `id-${sequence++}`, label }, currentAccount: false, plan: "max ×5",
    windows: [
      ...(windows.session ? [window("session", windows.session)] : []),
      ...(windows.weekly ? [window("weekly", windows.weekly)] : []),
      ...(windows.model ? [window("model", windows.model)] : []),
    ],
    ...overrides,
  };
}
const labels = (entries: ProviderLimits[]) => entries.map(entry => entry.account?.label);

it("puts the active account first even when it is out of usage", () => {
  const active = account("active", { session: [100, at(2)], weekly: [100, at(48)] }, { currentAccount: true });
  const fresh = account("fresh", { session: [0, at(5)], weekly: [0, at(100)] });
  expect(labels(orderAccountCards([fresh, active], NOW))).toEqual(["active", "fresh"]);
});

it("ranks usage left above waiting for a reset, however soon that reset is", () => {
  const nearlyOut = account("nearly out", { session: [99, at(4)], weekly: [30, at(100)] });
  const resetsInMinutes = account("resets in minutes", { session: [100, at(0.1)], weekly: [10, at(100)] });
  expect(labels(orderAccountCards([resetsInMinutes, nearlyOut], NOW))).toEqual(["nearly out", "resets in minutes"]);
});

it("weighs usage left by the plan's multiplier, so a bigger plan with less left can still win", () => {
  const max20 = account("max ×20 at 30% left", { session: [70, at(3)], weekly: [70, at(100)] }, { plan: "max ×20" });
  const max5 = account("max ×5 untouched", { session: [0, at(3)], weekly: [0, at(100)] }, { plan: "max ×5" });
  expect(labels(orderAccountCards([max5, max20], NOW))).toEqual(["max ×20 at 30% left", "max ×5 untouched"]);
  const max20Low = account("max ×20 at 20% left", { session: [80, at(3)], weekly: [80, at(100)] }, { plan: "max ×20" });
  expect(labels(orderAccountCards([max20Low, max5], NOW))).toEqual(["max ×5 untouched", "max ×20 at 20% left"]);
  // Codex: Pro is ×20 to Plus, as the badge says.
  const pro = account("pro", { weekly: [60, at(100)] }, { provider: "codex", plan: "pro" });
  const plus = account("plus", { weekly: [0, at(100)] }, { provider: "codex", plan: "plus" });
  expect(labels(orderAccountCards([plus, pro], NOW))).toEqual(["pro", "plus"]);
  // A Claude login whose tier the backend could not name is still a Max, worth five.
  const bareMax = account("bare max at 30% left", { weekly: [70, at(100)] }, { plan: "max" });
  const proUntouched = account("pro untouched", { weekly: [0, at(100)] }, { plan: "pro" });
  expect(labels(orderAccountCards([proUntouched, bareMax], NOW))).toEqual(["bare max at 30% left", "pro untouched"]);
});

it("measures usage left by the fuller of the two main windows, ignoring model buckets", () => {
  const sessionBound = account("session nearly full", { session: [90, at(1)], weekly: [10, at(100)] });
  const balanced = account("balanced", { session: [50, at(1)], weekly: [50, at(100)] });
  const opusFull = account("opus full", { session: [20, at(1)], weekly: [20, at(100)], model: [100, at(100)] });
  expect(labels(orderAccountCards([sessionBound, balanced, opusFull], NOW))).toEqual(["opus full", "balanced", "session nearly full"]);
});

it("ignores a full model bucket when working out when an exhausted account is usable again", () => {
  // Out on the 5-hour window for an hour; the Opus bucket resetting in four days is not what it waits for.
  const opusToo = account("opus bucket full too", { session: [100, at(1)], weekly: [30, at(100)], model: [100, at(96)] });
  const laterAnyway = account("weekly full for two days", { session: [10, at(1)], weekly: [100, at(48)] });
  expect(labels(orderAccountCards([laterAnyway, opusToo], NOW))).toEqual(["opus bucket full too", "weekly full for two days"]);
});

it("orders accounts that are out of usage by when they become usable again: the last full window's reset", () => {
  const weeklyFull = account("weekly full for two days", { session: [100, at(1)], weekly: [100, at(48)] });
  const sessionFull = account("session full for three hours", { session: [100, at(3)], weekly: [40, at(100)] });
  const noReset = account("no reset reported", { session: [100, null], weekly: [20, at(100)] });
  expect(labels(orderAccountCards([noReset, weeklyFull, sessionFull], NOW)))
    .toEqual(["session full for three hours", "weekly full for two days", "no reset reported"]);
});

it("treats a full window whose reset has passed as renewed, so the account has usage again", () => {
  const renewed = account("renewed", { session: [100, at(-1)], weekly: [30, at(100)] });
  const waiting = account("waiting", { session: [60, at(1)], weekly: [60, at(100)] });
  expect(labels(orderAccountCards([waiting, renewed], NOW))).toEqual(["renewed", "waiting"]);
});

it("puts accounts with unknown usage last and keeps the incoming order between equals", () => {
  const unknownA = account("unknown a", {});
  const unknownB = account("unknown b", {});
  // Equal capacity left: the backend's order (newest observation first) decides, not the resets.
  const twinA = account("twin a", { session: [40, at(50)], weekly: [40, at(100)] });
  const twinB = account("twin b", { session: [40, at(2)], weekly: [40, at(100)] });
  const out = account("out", { session: [100, at(2)], weekly: [0, at(100)] });
  expect(labels(orderAccountCards([unknownA, twinA, out, unknownB, twinB], NOW)))
    .toEqual(["twin a", "twin b", "out", "unknown a", "unknown b"]);
  const neverA = account("never a", { session: [100, null] });
  const neverB = account("never b", { session: [100, null] });
  expect(labels(orderAccountCards([neverB, neverA, out], NOW))).toEqual(["out", "never b", "never a"]);
});
