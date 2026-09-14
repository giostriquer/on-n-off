import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { TooltipButton } from "./TooltipButton";
afterEach(() => { cleanup(); vi.useRealTimers(); });
it("supports focus, Escape, and reopening without changing focus", () => {
  render(<TooltipButton label="Details" tooltip="Exact date">Badge</TooltipButton>);
  const trigger = screen.getByRole("button");
  act(() => trigger.focus());
  expect(trigger).toHaveAttribute("aria-describedby", screen.getByRole("tooltip").id);
  fireEvent.keyDown(document, {key:"Escape"});
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(trigger).toHaveFocus();
  fireEvent.click(trigger);
  expect(screen.getByRole("tooltip")).toBeVisible();
  fireEvent.blur(trigger);
  expect(screen.queryByRole("tooltip")).toBeNull();
});
it("keeps the tooltip open while crossing to it and dismisses on scroll", () => {
  vi.useFakeTimers();
  render(<TooltipButton label="Details" tooltip="Exact date">Badge</TooltipButton>);
  fireEvent.pointerEnter(screen.getByRole("button"));
  act(() => { vi.advanceTimersByTime(200); });
  fireEvent.pointerLeave(screen.getByRole("button"));
  fireEvent.pointerEnter(screen.getByRole("tooltip"));
  act(() => { vi.advanceTimersByTime(1000); });
  expect(screen.getByRole("tooltip")).toBeVisible();
  fireEvent.scroll(window);
  expect(screen.queryByRole("tooltip")).toBeNull();
});
it("does not leave a popup behind if unmounted during its hover delay", () => {
  vi.useFakeTimers();
  const view = render(<TooltipButton label="Details" tooltip="Exact date">Badge</TooltipButton>);
  fireEvent.pointerEnter(screen.getByRole("button"));
  view.unmount();
  act(() => { vi.advanceTimersByTime(500); });
  expect(screen.queryByRole("tooltip")).toBeNull();
});
