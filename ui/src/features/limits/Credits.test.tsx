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
  return { share: value("Workspace credits"), own: value("Credits"), text: document.body.textContent ?? "" };
}

describe("CreditsRows", () => {
  it("says how much of the member's share is left and when it resets", () => {
    const shown = rows(ZERO, share());

    expect(shown.share).toBe("17,000 of 25,000 left");
    expect(shown.text).toContain("resets Oct 1");
  });

  it("says a share that is used up is used up", () => {
    const shown = rows(ZERO, share({ used: "25000", reached: true }));

    expect(shown.share).toBe("Used up");
    expect(shown.text).toContain("all 25,000 used · resets Oct 1");
  });

  it("keeps the decimals an amount carries and never shows less than nothing left", () => {
    expect(rows(null, share({ limit: "25000.5", used: "8000.25" })).share).toBe("17,000.25 of 25,000.5 left");
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

  it("shows neither a share nor the own 0 once the share has reset, since what is used is no longer known", () => {
    const shown = rows(ZERO, share({ resetsAt: "2026-09-24T11:59:59Z" }));

    expect(shown.share).toBeNull();
    expect(shown.own).toBe("0");
  });

  it("shows nothing with neither a balance nor a share", () => {
    const { container } = render(<CreditsRows entry={{ credits: null, workspaceCredits: null }} now={NOW} />);

    expect(container.textContent).toBe("");
  });
});
