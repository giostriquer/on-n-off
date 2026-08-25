import { describe, expect, it } from "vitest";
import { visibleAgentIds, setAgentHidden, ALL_AGENTS, mergeAppSettings } from "./appSettings";

describe("appSettings", () => {
  it("defaults to every provider visible", () => {
    expect(visibleAgentIds([])).toEqual([...ALL_AGENTS]);
  });

  it("hides a provider from tabs without emptying the list", () => {
    expect(visibleAgentIds(["antigravity"])).toEqual(["claude", "codex", "cursor"]);
  });

  it("refuses to hide the last visible provider", () => {
    const hidden = setAgentHidden(["codex", "antigravity", "cursor"], "claude", true);
    expect(hidden).toEqual(["codex", "antigravity", "cursor"]);
    expect(visibleAgentIds(hidden)).toEqual(["claude"]);
  });

  it("can show a hidden provider again", () => {
    expect(setAgentHidden(["antigravity"], "antigravity", false)).toEqual([]);
  });

  it("enables automatic updates by default and preserves an opt-out", () => {
    expect((mergeAppSettings(null) as unknown as Record<string, unknown>).automaticUpdates).toBe(true);
    expect(
      (mergeAppSettings(JSON.parse('{"automaticUpdates":false}')) as unknown as Record<string, unknown>)
        .automaticUpdates,
    ).toBe(false);
  });

  it("keeps limit notifications off at a ten-minute default", () => {
    const settings = mergeAppSettings(null) as unknown as Record<string, unknown>;

    expect(settings.limitNotifications).toBe(false);
    expect(settings.limitsPollMinutes).toBe(10);
  });

  it("normalizes unsupported limit polling intervals", () => {
    const settings = mergeAppSettings(
      JSON.parse('{"limitNotifications":true,"limitsPollMinutes":7}'),
    ) as unknown as Record<string, unknown>;

    expect(settings.limitNotifications).toBe(true);
    expect(settings.limitsPollMinutes).toBe(10);
  });

  it("keeps the GitHub screen unscoped, quiet, and polling every sixty seconds by default", () => {
    const settings = mergeAppSettings(null) as unknown as Record<string, unknown>;

    expect(settings.githubScopes).toEqual([]);
    expect(settings.githubNotifications).toBe(false);
    expect(settings.githubPollSeconds).toBe(60);
  });

  it("normalizes unsupported GitHub polling intervals and keeps the scopes", () => {
    const settings = mergeAppSettings(
      JSON.parse('{"githubScopes":["org:acme"],"githubNotifications":true,"githubPollSeconds":45}'),
    ) as unknown as Record<string, unknown>;

    expect(settings.githubScopes).toEqual(["org:acme"]);
    expect(settings.githubNotifications).toBe(true);
    expect(settings.githubPollSeconds).toBe(60);
  });
});
