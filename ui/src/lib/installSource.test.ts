import { describe, expect, it } from "vitest";
import { githubRepoFromSource, isValidInstallInput, parseInstallSource } from "./installSource";

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

describe("githubRepoFromSource", () => {
  it("resolves shorthand and github.com urls, and nothing else", () => {
    expect(githubRepoFromSource(parseInstallSource("mattpocock/skills"))).toEqual({
      owner: "mattpocock",
      repo: "skills",
      ref: undefined,
    });
    expect(githubRepoFromSource(parseInstallSource("acme/tools@v1"))).toEqual({
      owner: "acme",
      repo: "tools",
      ref: "v1",
    });
    expect(githubRepoFromSource(parseInstallSource("https://github.com/acme/tools.git"))).toEqual({
      owner: "acme",
      repo: "tools",
      ref: undefined,
    });
    expect(githubRepoFromSource(parseInstallSource("https://github.com/acme/tools/tree/main"))).toEqual({
      owner: "acme",
      repo: "tools",
      ref: undefined,
    });
    expect(githubRepoFromSource(parseInstallSource("https://www.github.com/acme/tools/"))).toEqual({
      owner: "acme",
      repo: "tools",
      ref: undefined,
    });
    expect(githubRepoFromSource(parseInstallSource("HTTPS://GITHUB.COM/acme/tools"))).toEqual({
      owner: "acme",
      repo: "tools",
      ref: undefined,
    });
    expect(githubRepoFromSource(parseInstallSource("https://github.com/acme"))).toBeNull();
    expect(githubRepoFromSource(parseInstallSource("https://gitlab.com/acme/tools.git"))).toBeNull();
    expect(githubRepoFromSource(parseInstallSource("name@marketplace"))).toBeNull();
    expect(githubRepoFromSource(parseInstallSource("npx skills add acme/tools"))).toBeNull();
    expect(githubRepoFromSource(parseInstallSource(""))).toBeNull();
  });
});
