import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MuseIcon } from "./ProviderIcon";

describe("MuseIcon", () => {
  it("draws the official Muse squiggle logomark", () => {
    const { container } = render(<MuseIcon />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("viewBox")).toBe("0 0 100 100");
    const path = container.querySelector("path");
    expect(path?.getAttribute("d")).toMatch(/^M24\.6257 16\.1214C28\.2365 16\.1214 30\.8187 18\.9408 30\.8187 23\.3643/);
  });
});
