import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LeftRail } from "./LeftRail";

const counts = {
  plugins: { on: 3, total: 4 },
  skills: { on: 4, total: 6 },
  mcp: { on: 0, total: 0 },
};

describe("LeftRail", () => {
  it("renders Studio nav labels and hides master cut by default", () => {
    render(
      <LeftRail
        screen="overview"
        counts={counts}
        theme="dark"
        masterOn={false}
        masterNote="cuts every item for Claude"
        onScreen={() => undefined}
        onThemeChange={() => undefined}
        onMaster={() => undefined}
      />,
    );
    expect(screen.getByRole("navigation", { name: "Section" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Overview/i }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("button", { name: /Plugins 3\/4/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Skills 4\/6/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /MCP servers 0\/0/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Usage$/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Settings$/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Agent config/i })).toBeTruthy();
    expect(screen.getByRole("group", { name: "Theme" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Dark/i }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: /Light/i }).getAttribute("aria-pressed")).toBe("false");
    expect(screen.queryByText("Master cut")).toBeNull();
    expect(screen.queryByRole("button", { name: "Master cut" })).toBeNull();
  });

  it("shows master cut when the flag is on and switches theme", async () => {
    const user = userEvent.setup();
    const onScreen = vi.fn();
    const onMaster = vi.fn();
    const onThemeChange = vi.fn();
    render(
      <LeftRail
        screen="plugins"
        counts={counts}
        theme="dark"
        masterOn={true}
        masterNote="everything live on Claude"
        showMasterCut={true}
        onScreen={onScreen}
        onThemeChange={onThemeChange}
        onMaster={onMaster}
      />,
    );
    expect(screen.getByText("Master cut")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Master cut" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("everything live on Claude")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Skills 4\/6/i }));
    expect(onScreen).toHaveBeenCalledWith("skills");
    await user.click(screen.getByRole("button", { name: "Master cut" }));
    expect(onMaster).toHaveBeenCalledWith(false);
    await user.click(screen.getByRole("button", { name: /Light/i }));
    expect(onThemeChange).toHaveBeenCalledWith("light");
  });

  it("signals Usage intent before navigation", async () => {
    const user = userEvent.setup();
    const onUsageIntent = vi.fn();
    render(
      <LeftRail
        screen="overview"
        counts={counts}
        theme="dark"
        masterOn={false}
        masterNote=""
        onScreen={() => undefined}
        onThemeChange={() => undefined}
        onMaster={() => undefined}
        onUsageIntent={onUsageIntent}
      />,
    );

    const usage = screen.getByRole("button", { name: /^Usage$/i });
    await user.hover(usage);
    await user.tab();
    while (document.activeElement !== usage) {
      await user.tab();
    }
    expect(onUsageIntent).toHaveBeenCalledTimes(2);
  });
});
