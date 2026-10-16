import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MuseIcon, ProviderIcon } from "./ProviderIcon";

describe("MuseIcon", () => {
  it("draws the official Muse squiggle logomark", () => {
    const { container } = render(<MuseIcon />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("viewBox")).toBe("0 0 100 100");
    const path = container.querySelector("path");
    expect(path?.getAttribute("d")).toMatch(/^M24\.6257 16\.1214C28\.2365 16\.1214 30\.8187 18\.9408 30\.8187 23\.3643/);
  });
});

describe("ProviderIcon", () => {
  it.each([
    ["claude", "Claude", "0 0 24 24", "rgb(217, 119, 87)"],
    ["codex", "ChatGPT", "0 0 24 24", "var(--silkscreen)"],
    ["antigravity", "Antigravity", "13 14.5 85 85", "rgb(49, 134, 255)"],
    ["cursor", "Cursor", "0 0 466.73 532.09", "var(--silkscreen)"],
    ["muse", "Muse", "0 0 100 100", "rgb(0, 130, 251)"],
  ] as const)("draws %s's mark in its colour, titled for screen readers unless told otherwise", (provider, title, viewBox, color) => {
    const { container, rerender } = render(<ProviderIcon provider={provider} className="size-4" />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("viewBox")).toBe(viewBox);
    expect(svg.getAttribute("class")).toBe("size-4");
    expect(svg.style.color).toBe(color);
    expect(svg.getAttribute("role")).toBe("img");
    expect(container.querySelector("title")?.textContent).toBe(title);

    rerender(<ProviderIcon provider={provider} title="" />);
    expect(container.querySelector("svg")!.getAttribute("aria-hidden")).toBe("true");
    expect(container.querySelector("title")).toBeNull();
    expect(container.querySelector("svg")!.getAttribute("class")).toBe("size-3.5 shrink-0");
  });
});
