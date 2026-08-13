import { describe, expect, it } from "vitest";
import { isValidInstallInput, parseInstallSource } from "./installSource";

describe("parseInstallSource", () => {
  it("rejects empty input so Install stays disabled", () => {
    expect(parseInstallSource("")).toEqual({ error: "Use an HTTPS git URL or owner/repo." });
    expect(parseInstallSource("   ")).toEqual({ error: "Use an HTTPS git URL or owner/repo." });
    expect(isValidInstallInput("")).toBe(false);
  });

  it("rejects garbage and ssh urls", () => {
    expect(parseInstallSource("not a source")).toEqual({
      error: "Use an HTTPS git URL or owner/repo.",
    });
    expect(parseInstallSource("git@github.com:acme/tools.git")).toEqual({
      error: "Use an HTTPS git URL or owner/repo.",
    });
  });

  it("accepts owner/repo, owner/repo@ref, https git urls, plugin ids, and folders", () => {
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
  });
});
