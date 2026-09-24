import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LimitsCredits, LimitsWorkspaceCredits } from "$lib/limitsTypes";
import { CreditsRows } from "./Credits";

const NOW = Date.parse("2026-09-24T12:00:00Z");
const ZERO: LimitsCredits = { balance: "0", unlimited: false };

function share(overrides: Partial<LimitsWorkspaceCredits> = {}): LimitsWorkspaceCredits {
  return { limit: "25000", used: "8000", resetsAt: "2026-10-01T12:00:00Z", reached: false, ...overrides };
}

function rows(credits: LimitsCredits | null, workspaceCredits: LimitsWorkspaceCredits | null) {
  render(<CreditsRows entry={{ credits, workspaceCredits }} now={NOW} />);
  const value = (name: string) => screen.queryByRole("definition", { name })?.textContent ?? null;
  // The note is the row's second definition, the one that is not its named value.
  const shareRow = screen.queryByRole("definition", { name: "Workspace credits" })?.closest("dl");
  const note = shareRow ? (shareRow.querySelectorAll("dd")[1]?.textContent ?? null) : null;
  return { share: value("Workspace credits"), note, own: value("Credits") };
}

describe("CreditsRows", () => {
  it("says how much of the member's share is left and when it resets", () => {
    const shown = rows(ZERO, share());

    expect(shown.share).toBe("17,000 of 25,000 left");
    expect(shown.note).toBe("resets Oct 1");
  });

  it("says a share that is used up is used up", () => {
    const shown = rows(ZERO, share({ used: "25000", reached: true }));

    expect(shown.share).toBe("Used up");
    expect(shown.note).toBe("all 25,000 used · resets Oct 1");
  });

  it("gives no note for a share with no reset date that is not used up", () => {
    const shown = rows(null, share({ resetsAt: null }));

    expect(shown.share).toBe("17,000 of 25,000 left");
    expect(shown.note).toBeNull();
  });

  it("keeps the decimals an amount carries", () => {
    expect(rows(null, share({ limit: "25000.5", used: "8000.25" })).share).toBe("17,000.25 of 25,000.5 left");
  });

  it("rounds an amount to two decimals", () => {
    expect(rows(null, share({ limit: "10.125", used: "0" })).share).toBe("10.13 of 10.13 left");
  });

  it("shows nothing left rather than a negative amount when more than the share is used", () => {
    expect(rows(null, share({ limit: "100", used: "120" })).share).toBe("0 of 100 left");
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
    expect(rows(ZERO, null).own).toBe("0");
  });

  it("shows a share whose reset has passed as renewed, as a window is, and still leaves out the own 0", () => {
    const shown = rows(ZERO, share({ used: "25000", reached: true, resetsAt: "2026-09-23T10:00:00Z" }));

    expect(shown.share).toBe("25,000 of 25,000 left");
    expect(shown.note).toBe("reset 1d ago · Sep 23");
    expect(shown.own).toBeNull();
  });

  it("shows nothing with neither a balance nor a share", () => {
    const { container } = render(<CreditsRows entry={{ credits: null, workspaceCredits: null }} now={NOW} />);

    expect(container.textContent).toBe("");
  });
});
