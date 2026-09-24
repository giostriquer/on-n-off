import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LimitsCredits, LimitsWorkspaceCredits } from "$lib/limitsTypes";
import { CreditsRows } from "./Credits";

const NOW = Date.parse("2026-09-24T12:00:00Z");
const ZERO: LimitsCredits = { balance: "0", unlimited: false };

function share(overrides: Partial<LimitsWorkspaceCredits> = {}): LimitsWorkspaceCredits {
  return { limit: "25000", used: "8000", usedPercent: 32, resetsAt: "2026-10-01T12:00:00Z", reached: false, ...overrides };
}

function rows(credits: LimitsCredits | null, workspaceCredits: LimitsWorkspaceCredits | null) {
  render(<CreditsRows entry={{ provider: "codex", credits, workspaceCredits }} now={NOW} />);
  const meter = screen.queryByRole("meter", { name: "Workspace credits" });
  const noteId = meter?.getAttribute("aria-describedby");
  return {
    meter,
    /** What the bar reports as filled. */
    filled: meter?.getAttribute("aria-valuenow") ?? null,
    /** The figure beside the bar. */
    figure: meter?.nextElementSibling ?? null,
    /** The line under the label, which the bar names as its description. */
    note: noteId ? (document.getElementById(noteId)?.textContent ?? null) : null,
    own: screen.queryByRole("definition", { name: "Credits" })?.textContent ?? null,
  };
}

describe("CreditsRows", () => {
  it("fills a bar with the reader's figure and says what is left and when it resets", () => {
    const shown = rows(ZERO, share({ usedPercent: 40 }));

    expect(shown.filled).toBe("40");
    expect(shown.figure?.textContent).toBe("40%");
    expect(shown.note).toBe("17,000 of 25,000 left · resets Oct 1");
  });

  it("fills the bar and turns the figure red when the share is used up", () => {
    const shown = rows(ZERO, share({ used: "25000", usedPercent: 100, reached: true }));

    expect(shown.filled).toBe("100");
    expect(shown.figure).toHaveStyle({ color: "var(--trip)" });
    expect(shown.note).toBe("all 25,000 used · resets Oct 1");
  });

  it("says only that the limit is reached when a reached share's amounts show some left", () => {
    const shown = rows(ZERO, share({ used: "24000", usedPercent: 100, reached: true }));

    expect(shown.filled).toBe("100");
    expect(shown.note).toBe("limit reached · resets Oct 1");
  });

  it("fills the bar in Codex's accent while there is room", () => {
    const fill = rows(ZERO, share()).meter?.firstElementChild as HTMLElement | null;

    expect(fill?.style.width).toBe("32%");
    expect(fill?.style.backgroundColor).toBe("var(--silkscreen)");
  });

  it("says only what is left of a share with no reset date", () => {
    expect(rows(null, share({ resetsAt: null })).note).toBe("17,000 of 25,000 left");
  });

  it("keeps the decimals an amount carries", () => {
    expect(rows(null, share({ limit: "25000.5", used: "8000.25" })).note).toBe("17,000.25 of 25,000.5 left · resets Oct 1");
  });

  it("rounds an amount to two decimals", () => {
    expect(rows(null, share({ limit: "10.125", used: "0" })).note).toBe("10.13 of 10.13 left · resets Oct 1");
  });

  it("shows nothing left rather than a negative amount when more than the share is used", () => {
    const shown = rows(null, share({ limit: "100", used: "120", usedPercent: 100 }));

    expect(shown.note).toBe("0 of 100 left · resets Oct 1");
    expect(shown.filled).toBe("100");
  });

  it("leaves out an own balance of 0 beside a share, which says what the member can use", () => {
    expect(rows(ZERO, share()).own).toBeNull();
  });

  it("keeps an own balance that says something", () => {
    expect(rows({ balance: "3", unlimited: false }, share()).own).toBe("3");
  });

  it("keeps an unlimited balance beside a share", () => {
    expect(rows({ balance: "0", unlimited: true }, share()).own).toBe("Unlimited");
  });

  it("keeps an own balance of 0 when there is no share", () => {
    const shown = rows(ZERO, null);

    expect(shown.own).toBe("0");
    expect(shown.meter).toBeNull();
  });

  it("shows a share whose reset has passed as renewed, as a window is, and still leaves out the own 0", () => {
    const shown = rows(ZERO, share({ used: "25000", usedPercent: 100, reached: true, resetsAt: "2026-09-23T10:00:00Z" }));

    expect(shown.filled).toBe("0");
    expect(shown.figure?.textContent).toBe("0%");
    expect(shown.note).toBe("25,000 of 25,000 left · reset 1d ago · Sep 23");
    expect(shown.own).toBeNull();
  });

  it("shows nothing with neither a balance nor a share", () => {
    const { container } = render(<CreditsRows entry={{ provider: "codex", credits: null, workspaceCredits: null }} now={NOW} />);

    expect(container.textContent).toBe("");
  });
});
