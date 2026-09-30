import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Segmented } from "./Segmented";

const OPTIONS = [
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month", disabled: true },
] as const;

describe("Segmented", () => {
  it("presses the chosen options and reports a press, as a named group", () => {
    const onPress = vi.fn();
    render(<Segmented ariaLabel="Range" options={OPTIONS} pressed={(value) => value === "week"} onPress={onPress} />);

    const group = screen.getByRole("group", { name: "Range" });
    expect(within(group).getByRole("button", { name: "Week" })).toHaveAttribute("aria-pressed", "true");
    expect(within(group).getByRole("button", { name: "Day" })).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(within(group).getByRole("button", { name: "Day" }));
    expect(onPress).toHaveBeenCalledWith("day");
  });

  it("presses several options at once for independent choices", () => {
    render(<Segmented ariaLabel="Lists" options={OPTIONS} pressed={(value) => value !== "month"} onPress={() => {}} />);

    expect(screen.getAllByRole("button", { pressed: true }).map((button) => button.textContent)).toEqual(["Day", "Week"]);
  });

  it("never reports a disabled option, nor any option while the whole control is disabled", () => {
    const onPress = vi.fn();
    const { rerender } = render(<Segmented ariaLabel="Range" options={OPTIONS} pressed={() => false} onPress={onPress} />);

    fireEvent.click(screen.getByRole("button", { name: "Month" }));
    rerender(<Segmented ariaLabel="Range" options={OPTIONS} pressed={() => false} onPress={onPress} disabled />);
    fireEvent.click(screen.getByRole("button", { name: "Day" }));

    expect(onPress).not.toHaveBeenCalled();
  });

  it("can be named by a visible label instead", () => {
    render(<><span id="edge-label">Edge</span><Segmented ariaLabelledBy="edge-label" options={OPTIONS} pressed={() => false} onPress={() => {}} /></>);

    expect(screen.getByRole("group", { name: "Edge" })).toBeTruthy();
  });
});
