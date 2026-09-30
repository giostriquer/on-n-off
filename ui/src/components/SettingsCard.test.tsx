import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CardToggle, SettingRow, SettingsCard } from "./SettingsCard";

describe("SettingsCard", () => {
  it("is a region named for its setting, with its title, what it does, its switch and its rows", () => {
    const onToggle = vi.fn();
    render(
      <SettingsCard
        label="Accounts"
        title="Accounts"
        description="Automatically save accounts I sign in to."
        control={<CardToggle caption="Auto-save" on={false} ariaLabel="Automatically save accounts I sign in to" onToggle={onToggle} />}
      >
        <SettingRow>Saved accounts appear on Limits.</SettingRow>
      </SettingsCard>,
    );

    const card = screen.getByRole("region", { name: "Accounts" });
    expect(within(card).getByRole("heading", { name: "Accounts" })).toBeTruthy();
    expect(card).toHaveTextContent("Automatically save accounts I sign in to.");
    expect(card).toHaveTextContent("Auto-save");
    expect(card).toHaveTextContent("Saved accounts appear on Limits.");
    const toggle = within(card).getByRole("button", { name: "Automatically save accounts I sign in to" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("keeps its toggle still while it is busy", () => {
    const onToggle = vi.fn();
    render(<CardToggle caption="Notify" on busy ariaLabel="Notify" onToggle={onToggle} />);

    fireEvent.click(screen.getByRole("button", { name: "Notify" }));

    expect(onToggle).not.toHaveBeenCalled();
  });
});
