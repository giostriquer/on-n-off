import { describe, expect, it } from "vitest";
import { isValidInstallInput, parseInstallSource } from "./installSource";

const INVALID = "Use an HTTPS git URL, owner/repo, name@marketplace, or npx skills add.";

describe("parseInstallSource", () => {
  it("rejects empty input so Install stays disabled", () => {
    expect(parseInstallSource("")).toEqual({ error: INVALID });
    expect(parseInstallSource("   ")).toEqual({ error: INVALID });
    expect(isValidInstallInput("")).toBe(false);
  });

  it("rejects garbage and ssh urls", () => {
    expect(parseInstallSource("not a source")).toEqual({ error: INVALID });
    expect(parseInstallSource("git@github.com:acme/tools.git")).toEqual({ error: INVALID });
  });

  it("accepts owner/repo, owner/repo@ref, https git urls, plugin ids, folders, and npx skills", () => {
    expect(parseInstallSource("acme/tools")).toEqual({
      kind: "github",
      owner: "acme",
      repo: "tools",
      ref: undefined,
    });
    expect(parseInstallSource("acme/tools@v1")).toEqual({
      kind: "github",
      owner: "acme",
      repo: "tools",
      ref: "v1",
    });
    expect(parseInstallSource("https://github.com/acme/tools.git")).toEqual({
      kind: "git-url",
      value: "https://github.com/acme/tools.git",
    });
    expect(parseInstallSource("workbench@workshop")).toEqual({
      kind: "plugin",
      id: "workbench@workshop",
    });
    expect(parseInstallSource(String.raw`E:\dev\dummy-plugin`)).toEqual({
      kind: "folder",
      value: String.raw`E:\dev\dummy-plugin`,
    });
    expect(parseInstallSource("npx -y skills add vercel-labs/agent-skills -g --skill web-design")).toEqual({
      kind: "npx-skills",
      source: "vercel-labs/agent-skills",
      skill: "web-design",
    });
    expect(parseInstallSource("skills add anthropics/skills")).toEqual({
      kind: "npx-skills",
      source: "anthropics/skills",
    });
  });
});
