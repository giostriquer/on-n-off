import { expect, it } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import { accountCards } from "./accountCards";
import { presentLimitAccount } from "./limitPresentation";

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
// The Codex reader drops the windows no surface shows before a card leaves the backend, so a card
// and the account list count the same windows: every one it carries. A window a surface once hid
// replaces the account's history in the list, and the card shows it as remembered usage.
it("counts every window a card carries, as the card itself does", () => {
  const onlyAWindowOnceHidden = {
    ...scoped,
    windows: [{ ...scoped.windows[0], id: "extra:codex_bengalfox", label: "Weekly · GPT-5.3-Codex-Spark", kind: "model" as const }],
  };

  expect(accountCards([onlyAWindowOnceHidden, legacy], [profile]).entries).toEqual([onlyAWindowOnceHidden]);
  expect(presentLimitAccount(onlyAWindowOnceHidden, "fallback").remembered).toBe(true);
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
