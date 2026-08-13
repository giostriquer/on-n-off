import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import SkillRow from "./SkillRow.svelte";
import type { SkillDto } from "./types";

const locked: SkillDto = {
  id: "brainstorming",
  pluginId: "workbench@workshop",
  name: "brainstorming",
  description: "Turn ideas into designs",
  enabled: true,
  togglable: false,
};

const togglable: SkillDto = {
  id: "statusline",
  pluginId: null,
  name: "statusline",
  description: "Custom status line",
  enabled: true,
  togglable: true,
};

describe("SkillRow", () => {
  it("does not render a rocker for locked plugin skills", () => {
    render(SkillRow, { props: { skill: locked, onToggle: () => undefined } });
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("Claude only enables this with the whole plugin.")).toBeTruthy();
  });

  it("renders a pressed rocker for togglable skills", () => {
    render(SkillRow, { props: { skill: togglable, onToggle: () => undefined } });
    const rocker = screen.getByRole("button", { name: "statusline on" });
    expect(rocker.getAttribute("aria-pressed")).toBe("true");
  });
});
