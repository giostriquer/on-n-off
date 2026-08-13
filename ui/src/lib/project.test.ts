import { describe, expect, it } from "vitest";
import {
  looksLikeFolderPath,
  mergeProjects,
  normalizeProjectKey,
  projectFromPath,
  projectLabel,
  sameProjectPath,
  scopeChip,
} from "./project";

describe("project", () => {
  it("normalizes windows paths and labels", () => {
    expect(normalizeProjectKey("E:\\dev\\on-n-off\\")).toBe("e:/dev/on-n-off");
    expect(projectLabel("E:\\dev\\on-n-off")).toBe("on-n-off");
    expect(projectLabel("E:\\dev\\")).toBe("dev");
    expect(sameProjectPath("E:\\dev\\app", "e:/dev/app/")).toBe(true);
  });

  it("merges recognized and picked folders without duplicates", () => {
    const merged = mergeProjects(
      [projectFromPath("E:\\dev\\on-n-off"), projectFromPath("E:\\dev\\conoswiki")],
      [projectFromPath("E:/dev/on-n-off/"), projectFromPath("D:\\tmp\\scratch")],
    );
    expect(merged.map((project) => project.label)).toEqual(["conoswiki", "on-n-off", "scratch"]);
  });

  it("detects pasted folder paths and chips real local counts", () => {
    expect(looksLikeFolderPath(String.raw`E:\dev\on-n-off`)).toBe(true);
    expect(looksLikeFolderPath("~/work/app")).toBe(true);
    expect(looksLikeFolderPath("conoswiki")).toBe(false);
    expect(scopeChip(null)).toBe("global config");
    expect(scopeChip({ id: "e:/dev/app", label: "app", path: "E:/dev/app", skillCount: 3, mcpCount: 1 })).toBe(
      "3 local skills · 1 project mcps",
    );
  });
});
