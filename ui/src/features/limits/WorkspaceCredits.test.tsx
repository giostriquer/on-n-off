import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LimitsWorkspaceCredits } from "$lib/limitsTypes";
import { showsOwnCredits, WorkspaceCreditsRow } from "./WorkspaceCredits";

const NOW = Date.parse("2026-09-24T12:00:00Z");

function share(overrides: Partial<LimitsWorkspaceCredits> = {}): LimitsWorkspaceCredits {
  return { limit: "25000", used: "8000", remainingPercent: 68, resetsAt: "2026-10-01T12:00:00Z", reached: false, ...overrides };
}

function row() {
  return {
    value: screen.getByRole("definition", { name: "Workspace credits" }).textContent,
    text: document.body.textContent ?? "",
  };
}

describe("WorkspaceCreditsRow", () => {
  it("says how much of the member's share is left and when it resets", () => {
    render(<WorkspaceCreditsRow share={share()} now={NOW} />);

    expect(row().value).toBe("17,000 of 25,000 left");
    expect(row().text).toContain("resets Oct 1");
  });

  it("says a share that is used up is used up", () => {
    render(<WorkspaceCreditsRow share={share({ used: "25000", remainingPercent: 0, reached: true })} now={NOW} />);

    expect(row().value).toBe("Used up");
    expect(row().text).toContain("all 25,000 used · resets Oct 1");
  });

  it("keeps the decimals an amount carries", () => {
    render(<WorkspaceCreditsRow share={share({ limit: "25000.5", used: "8000.25" })} now={NOW} />);

    expect(row().value).toBe("17,000.25 of 25,000.5 left");
  });

  it("falls back to the share left when the amounts are not numbers", () => {
    render(<WorkspaceCreditsRow share={share({ limit: "n/a", used: "n/a", resetsAt: null })} now={NOW} />);

    expect(row().value).toBe("68% left");
    expect(row().text).not.toContain("resets");
  });

  it("shows nothing once the share has reset, since what is used is no longer known", () => {
    const { container } = render(<WorkspaceCreditsRow share={share({ resetsAt: "2026-09-24T11:59:59Z" })} now={NOW} />);

    expect(container.textContent).toBe("");
  });

  it("shows nothing without a share", () => {
    const { container } = render(<WorkspaceCreditsRow share={null} now={NOW} />);

    expect(container.textContent).toBe("");
  });
});

describe("showsOwnCredits", () => {
  const zero = { balance: "0", unlimited: false };

  it("leaves out a zero balance when the credits are the workspace's", () => {
    expect(showsOwnCredits({ credits: zero, workspaceCredits: share() })).toBe(false);
  });

  it("keeps a balance that says something", () => {
    expect(showsOwnCredits({ credits: { balance: "3", unlimited: false }, workspaceCredits: share() })).toBe(true);
    expect(showsOwnCredits({ credits: { balance: "0", unlimited: true }, workspaceCredits: share() })).toBe(true);
    expect(showsOwnCredits({ credits: zero, workspaceCredits: null })).toBe(true);
  });

  it("has nothing to show without a balance", () => {
    expect(showsOwnCredits({ credits: null, workspaceCredits: share() })).toBe(false);
  });
});
