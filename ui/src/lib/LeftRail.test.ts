import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import LeftRail from "./LeftRail.svelte";

const counts = {
  plugins: { on: 3, total: 4 },
  skills: { on: 4, total: 6 },
  mcp: { on: 0, total: 0 },
};

describe("LeftRail", () => {
  it("renders Studio nav labels and hides master cut by default", () => {
    render(LeftRail, {
      props: {
        screen: "overview",
        counts,
        masterOn: false,
        masterNote: "cuts every item for Claude",
        onScreen: () => undefined,
        onMaster: () => undefined,
      },
    });
    expect(screen.getByRole("navigation", { name: "Section" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Overview/i }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("button", { name: /Plugins 3\/4/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Skills 4\/6/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /MCP servers 0\/0/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Usage$/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Agent config/i })).toBeTruthy();
    expect(screen.queryByText("Master cut")).toBeNull();
    expect(screen.queryByRole("button", { name: "Master cut" })).toBeNull();
  });

  it("shows master cut when the flag is on", async () => {
    const user = userEvent.setup();
    const onScreen = vi.fn();
    const onMaster = vi.fn();
    render(LeftRail, {
      props: {
        screen: "plugins",
        counts,
        masterOn: true,
        masterNote: "everything live on Claude",
        showMasterCut: true,
        onScreen,
        onMaster,
      },
    });
    expect(screen.getByText("Master cut")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Master cut" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("everything live on Claude")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Skills 4\/6/i }));
    expect(onScreen).toHaveBeenCalledWith("skills");
    await user.click(screen.getByRole("button", { name: "Master cut" }));
    expect(onMaster).toHaveBeenCalledWith(false);
  });
});
