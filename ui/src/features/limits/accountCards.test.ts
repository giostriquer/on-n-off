import { expect, it } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import type { ProviderLimits } from "$lib/limitsTypes";
import rustProfiles from "../../../../src-tauri/src/accounts/fixtures/listed-profiles.json";
import { accountCards } from "./accountCards";

const profile: SavedProfile = {
  id: "saved", observationId: "profile:user-team", legacyId: "team", identity: { provider: "codex", userId: "user", workspaceId: "team" },
  email: "person@example.com", label: "person@example.com", savedAt: "2026-09-13T12:00:00Z", active: false, loginState: { state: "usable" },
};
const scoped: ProviderLimits = {
  provider: "codex", status: "ok", account: { id: profile.observationId, label: profile.email }, currentAccount: false,
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
  { ...scoped, status: "failed" as const },
  { ...scoped, status: "signedOut" as const },
  { ...scoped, account: { id: "profile:other-user", label: profile.email } },
  { ...scoped, account: { id: profile.observationId, label: "other@example.com" } },
])("retains history when scoped usage cannot be verified: %j", candidate => {
  expect(accountCards([candidate, legacy], [profile]).entries).toEqual([candidate, legacy]);
});
it.each([
  { ...scoped, windows: [] },
  { ...scoped, windows: [], resetCredits: { availableCount: 1, nextExpiresAt: null } },
  { ...scoped, windows: [{ ...scoped.windows[0], id: "extra:codex_bengalfox", label: "Weekly · GPT-5.3-Codex-Spark", kind: "model" as const }] },
])("folds history behind a scoped read that answered, whatever it observed, as Rust's own fold does: %j", answered => {
  expect(accountCards([answered, legacy], [profile]).entries).toEqual([answered]);
});
it("does not infer identity from email without a saved profile", () => {
  expect(accountCards([scoped, legacy], []).entries).toEqual([scoped, legacy]);
  expect(accountCards([legacy], [profile]).entries).toEqual([legacy]);
  expect(accountCards([scoped, legacy], [{ ...profile, email: " " }]).entries).toEqual([scoped, legacy]);
});
it("reconciles Claude using its user ID and normalizes display emails", () => {
  const claude = { ...scoped, provider: "claude" as const, account: { id: "profile:claude", label: " Person@Example.com " } };
  const old = { ...legacy, provider: "claude" as const, account: { id: "user", label: profile.email } };
  expect(accountCards([claude, old], [{ ...profile, observationId: "profile:claude", legacyId: "user", identity: { ...profile.identity, provider: "claude" } }]).entries).toEqual([claude]);
});
it("folds a saved Muse login's email-keyed history into its scoped card", () => {
  const muse: SavedProfile = {
    ...profile, id: "saved-muse", observationId: "profile:muse-you", legacyId: "email:you@example.com", email: "You@Example.com",
    identity: { provider: "muse", userId: "you@example.com", workspaceId: "" },
  };
  const scopedMuse: ProviderLimits = { ...scoped, provider: "muse", account: { id: muse.observationId, label: "you@example.com" } };
  const emailHistory: ProviderLimits = { ...legacy, provider: "muse", account: { id: "email:you@example.com", label: "you@example.com" } };
  const otherEmail: ProviderLimits = { ...emailHistory, account: { id: "email:other@example.com", label: "other@example.com" } };

  const shown = accountCards([scopedMuse, emailHistory, otherEmail], [muse]);

  expect(shown.entries).toEqual([scopedMuse, otherEmail]);
  expect(shown.legacyAccounts.get(muse.observationId)).toEqual([{ id: "email:you@example.com", email: "you@example.com" }]);
});
it("folds the history the saved profile names as its legacy id, never one rebuilt from its identity", () => {
  const renamed = { ...profile, legacyId: "renamed-team" };
  const named = { ...legacy, account: { id: "renamed-team", label: profile.email } };

  const shown = accountCards([scoped, named, legacy], [renamed]);

  expect(shown.entries).toEqual([scoped, legacy]);
  expect(shown.legacyAccounts.get(profile.observationId)).toEqual([{ id: "renamed-team", email: profile.email }]);
});
it("folds nothing into a saved profile that names no legacy history", () => {
  const { legacyId: _, ...unnamed } = profile;

  expect(accountCards([scoped, legacy], [unnamed]).entries).toEqual([scoped, legacy]);
});
it.each(Object.entries(rustProfiles))("folds %s history by the legacy id Rust lists on the profile", (_, wire) => {
  const listed: SavedProfile = { ...profile, ...wire, identity: wire.identity as SavedProfile["identity"] };
  const card: ProviderLimits = { ...scoped, provider: listed.identity.provider, account: { id: wire.observationId, label: profile.email } };
  const old: ProviderLimits = { ...legacy, provider: listed.identity.provider, account: { id: wire.legacyId, label: profile.email } };

  expect(accountCards([card, old], [listed]).entries).toEqual([card]);
});
