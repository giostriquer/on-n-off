import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatShortDate } from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { BankedResetsRow, UseBankedReset } from "./BankedResets";

const consumeCodexResetCredit = vi.hoisted(() => vi.fn());
vi.mock("$lib/api", () => ({ consumeCodexResetCredit }));

const NOW = Date.parse("2026-08-17T20:00:00Z");
const OBSERVED = "2026-08-17T20:00:00Z";

function weekly(usedPercent: number): LimitWindow {
  return { id: "secondary", label: "Weekly · all models", kind: "weekly", usedPercent, resetsAt: "2026-08-22T00:00:00Z", observedAt: OBSERVED };
}

function session(usedPercent: number): LimitWindow {
  return { id: "primary", label: "5 hour · all models", kind: "session", usedPercent, resetsAt: "2026-08-17T23:00:00Z", observedAt: OBSERVED };
}

function codex(overrides: Partial<ProviderLimits> = {}): ProviderLimits {
  return {
    provider: "codex",
    status: "ok",
    currentAccount: true,
    account: { id: "acct-work", label: "work@codex.example" },
    windows: [weekly(40), session(12)],
    resetCredits: { availableCount: 1, nextExpiresAt: "2026-08-29T15:00:00Z" },
    ...overrides,
  };
}

function renderButton(entry: ProviderLimits) {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const view = render(
    <QueryClientProvider client={client}>
      <UseBankedReset entry={entry} now={NOW} />
    </QueryClientProvider>,
  );
  return { ...view, invalidate };
}

function useReset() {
  return screen.getByRole("button", { name: "Use banked reset" });
}

beforeEach(() => {
  consumeCodexResetCredit.mockReset().mockResolvedValue("reset");
});

describe("BankedResetsRow", () => {
  it("reads as one more row: the count, and when the next banked reset expires", () => {
    render(<BankedResetsRow resetCredits={{ availableCount: 2, nextExpiresAt: "2026-08-29T15:00:00Z" }} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(screen.getByText(`next expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`)).toBeTruthy();
  });

  it("drops the note when the expiry is unknown", () => {
    render(<BankedResetsRow resetCredits={{ availableCount: 1, nextExpiresAt: null }} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("1");
    expect(screen.queryByText(/expires/)).toBeNull();
  });

  it("stays out of the card when nothing is banked", () => {
    for (const resetCredits of [null, undefined, { availableCount: 0, nextExpiresAt: null }]) {
      const { container, unmount } = render(<BankedResetsRow resetCredits={resetCredits} now={NOW} />);
      expect(container.innerHTML).toBe("");
      unmount();
    }
  });
});

describe("UseBankedReset", () => {
  it("is offered only on the live, signed-in Codex account that has a reset banked", () => {
    for (const entry of [
      codex({ currentAccount: false }),
      codex({ status: "failed" }),
      codex({ account: null }),
      codex({ resetCredits: { availableCount: 0, nextExpiresAt: null } }),
      codex({ resetCredits: null }),
      codex({ provider: "claude" }),
    ]) {
      const { unmount } = renderButton(entry);
      expect(screen.queryByRole("button", { name: "Use banked reset" })).toBeNull();
      unmount();
    }

    renderButton(codex());
    expect(useReset()).toBeTruthy();
  });

  it("spends the reset straight away once less than 5% of usage is left", async () => {
    const { invalidate } = renderButton(codex({ windows: [weekly(96), session(12)] }));

    fireEvent.click(useReset());

    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledTimes(1));
    const [accountId, attempt] = consumeCodexResetCredit.mock.calls[0];
    expect(accountId).toBe("acct-work");
    expect(attempt).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["limits", "codex"] });
  });

  it("asks before spending a reset while usage is still left, and says how much is left", async () => {
    renderButton(codex());

    fireEvent.click(useReset());
    const dialog = screen.getByRole("alertdialog", { name: "Use a banked reset now?" });
    // The dialog names the account the reset lands on.
    expect(within(dialog).getByText(/^work@codex\.example still has 60% of its Codex usage left/)).toBeTruthy();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();

    fireEvent.click(useReset());
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Use reset anyway" }));
    await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("names the account the way its card does", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <UseBankedReset entry={codex()} label="saved@codex.example" now={NOW} />
      </QueryClientProvider>,
    );

    fireEvent.click(useReset());

    expect(within(screen.getByRole("alertdialog")).getByText(/^saved@codex\.example still has 60%/)).toBeTruthy();
  });

  it("asks when on-n-off cannot tell how much usage is left", () => {
    renderButton(codex({ windows: [] }));

    fireEvent.click(useReset());

    expect(within(screen.getByRole("alertdialog")).getByText(/can't tell how much Codex usage/)).toBeTruthy();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();
  });

  it.each([
    ["nothingToReset", "Nothing to reset: this account's usage is already at 0%."],
    ["noCredit", "This account has no banked reset left."],
    ["alreadyRedeemed", "That banked reset was already used."],
  ])("explains a %s outcome", async (outcome, message) => {
    consumeCodexResetCredit.mockResolvedValue(outcome);
    renderButton(codex({ windows: [weekly(99)] }));

    fireEvent.click(useReset());

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(message));
  });

  it("shows why a reset could not be spent", async () => {
    consumeCodexResetCredit.mockRejectedValue({
      kind: "message",
      message: "The signed-in Codex account changed. Refresh Limits and try again.",
    });
    renderButton(codex({ windows: [weekly(99)] }));

    fireEvent.click(useReset());

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toBe("The signed-in Codex account changed. Refresh Limits and try again."),
    );
  });
});
