import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LimitsCredits, LimitsCreditsSpent, LimitsWorkspaceCredits } from "$lib/limitsTypes";
import { CreditsRows } from "./Credits";
import type { CardFigures } from "./limitCards";

const NOW = Date.parse("2026-09-24T12:00:00Z");
const ZERO: LimitsCredits = { balance: "0", unlimited: false };

function share(overrides: Partial<LimitsWorkspaceCredits> = {}): LimitsWorkspaceCredits {
  return { limit: "25000", used: "8000", usedPercent: 32, resetsAt: "2026-10-01T12:00:00Z", reached: false, ...overrides };
}

/** Which figures to show is the card model's call (`limitCards.test.ts`); these are how they read. */
function rows(figures: Partial<Pick<CardFigures, "ownBalance" | "workspaceShare" | "creditsSpent">>) {
  render(<CreditsRows figures={{ ownBalance: null, workspaceShare: null, creditsSpent: null, ...figures }} provider="codex" now={NOW} />);
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
    const shown = rows({ workspaceShare: share({ usedPercent: 40 }) });

    expect(shown.filled).toBe("40");
    expect(shown.figure?.textContent).toBe("40%");
    expect(shown.note).toBe("17,000 of 25,000 left · resets Oct 1");
  });

  it("fills the bar and turns the figure red when the share is used up", () => {
    const shown = rows({ workspaceShare: share({ used: "25000", usedPercent: 100, reached: true }) });

    expect(shown.filled).toBe("100");
    expect(shown.figure).toHaveStyle({ color: "var(--trip)" });
    expect(shown.note).toBe("all 25,000 used · resets Oct 1");
  });

  it("says only that the limit is reached when a reached share's amounts show some left", () => {
    const shown = rows({ workspaceShare: share({ used: "24000", usedPercent: 100, reached: true }) });

    expect(shown.filled).toBe("100");
    expect(shown.note).toBe("limit reached · resets Oct 1");
  });

  it("fills the bar in Codex's accent while there is room", () => {
    const fill = rows({ workspaceShare: share() }).meter?.firstElementChild as HTMLElement | null;

    expect(fill?.style.width).toBe("32%");
    expect(fill?.style.backgroundColor).toBe("var(--silkscreen)");
  });

  it("says only what is left of a share with no reset date", () => {
    expect(rows({ workspaceShare: share({ resetsAt: null }) }).note).toBe("17,000 of 25,000 left");
  });

  it("keeps the decimals an amount carries", () => {
    expect(rows({ workspaceShare: share({ limit: "25000.5", used: "8000.25" }) }).note).toBe("17,000.25 of 25,000.5 left · resets Oct 1");
  });

  it("rounds an amount to two decimals", () => {
    expect(rows({ workspaceShare: share({ limit: "10.125", used: "0" }) }).note).toBe("10.13 of 10.13 left · resets Oct 1");
  });

  it("shows nothing left rather than a negative amount when more than the share is used", () => {
    const shown = rows({ workspaceShare: share({ limit: "100", used: "120", usedPercent: 100 }) });

    expect(shown.note).toBe("0 of 100 left · resets Oct 1");
    expect(shown.filled).toBe("100");
  });

  it("shows the own balance it is given, with no meter", () => {
    const shown = rows({ ownBalance: ZERO });

    expect(shown.own).toBe("0");
    expect(shown.meter).toBeNull();
  });

  it("shows an unlimited balance as unlimited", () => {
    expect(rows({ ownBalance: { balance: "0", unlimited: true }, workspaceShare: share() }).own).toBe("Unlimited");
  });

  it("shows a share whose reset has passed as renewed, as a window is", () => {
    const shown = rows({ workspaceShare: share({ used: "25000", usedPercent: 100, reached: true, resetsAt: "2026-09-23T10:00:00Z" }) });

    expect(shown.filled).toBe("0");
    expect(shown.figure?.textContent).toBe("0%");
    expect(shown.note).toBe("25,000 of 25,000 left · reset 1d ago · Sep 23");
  });

  it("shows nothing with neither a balance nor a share", () => {
    const { container } = render(<CreditsRows figures={{ ownBalance: null, workspaceShare: null, creditsSpent: null }} provider="codex" now={NOW} />);

    expect(container.textContent).toBe("");
  });

  it("shows what a business member spent, and beside it the own balance it is given", () => {
    const spent: LimitsCreditsSpent = { last7Days: 18303.4, last30Days: 20299.7, updatedAt: null };
    const shown = rows({ ownBalance: { balance: "3", unlimited: false }, creditsSpent: spent });

    const value = screen.getByRole("definition", { name: "Credits spent" });
    expect(value.textContent).toBe("18,303.4");
    expect(value.closest("dl")?.textContent).toContain("last 7 days · 20,299.7 in 30 days");
    expect(shown.own).toBe("3");
  });
});
