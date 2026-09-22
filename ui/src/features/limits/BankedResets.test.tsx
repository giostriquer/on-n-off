import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatShortDate } from "$lib/limitsFormat";
import type { LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { BankedResetsRow, ResetOfferRow, UseBankedReset } from "./BankedResets";

const consumeCodexResetCredit = vi.hoisted(() => vi.fn());
vi.mock("$lib/api", () => ({ consumeCodexResetCredit }));

const NOW = Date.parse("2026-08-17T20:00:00Z");
const OBSERVED = "2026-08-17T20:00:00Z";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

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

type ButtonProps = { entry: ProviderLimits; current?: boolean; disabled?: boolean; label?: string };

function button({ entry, current = true, disabled = false, label = "work@codex.example" }: ButtonProps) {
  return <UseBankedReset entry={entry} label={label} current={current} now={NOW} disabled={disabled} />;
}

function useReset() {
  return screen.getByRole("button", { name: "Use banked reset" });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(() => {
  consumeCodexResetCredit.mockReset().mockResolvedValue("reset");
});

describe("BankedResetsRow", () => {
  it("reads as one more row: the count, and when the next banked reset expires", () => {
    render(<BankedResetsRow provider="codex" resetCredits={{ availableCount: 2, nextExpiresAt: "2026-08-29T15:00:00Z" }} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(screen.getByText(`next expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`)).toBeTruthy();
  });

  it("says a lone reset expires, not the next one", () => {
    render(<BankedResetsRow provider="codex" resetCredits={{ availableCount: 1, nextExpiresAt: "2026-08-29T15:00:00Z" }} now={NOW} />);

    expect(screen.getByText(`expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`)).toBeTruthy();
  });

  it("drops the note when the expiry is unknown or already past", () => {
    for (const nextExpiresAt of [null, "2026-08-17T19:00:00Z"]) {
      const { unmount } = render(<BankedResetsRow provider="codex" resetCredits={{ availableCount: 1, nextExpiresAt }} now={NOW} />);
      expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("1");
      expect(screen.queryByText(/expires/)).toBeNull();
      unmount();
    }
  });

  it("tells a Claude account where its reset is spent, since on-n-off only reports it", () => {
    render(<BankedResetsRow provider="claude" resetCredits={{ availableCount: 1, nextExpiresAt: "2026-08-29T15:00:00Z" }} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("1");
    expect(screen.getByText(`expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")} · /limit-reset in Claude Code`)).toBeTruthy();
  });

  it("still names /limit-reset when a Claude reset's expiry is unknown", () => {
    render(<BankedResetsRow provider="claude" resetCredits={{ availableCount: 1, nextExpiresAt: null }} now={NOW} />);

    expect(screen.getByText("/limit-reset in Claude Code")).toBeTruthy();
  });

  it("stays out of the card when nothing is banked", () => {
    for (const resetCredits of [null, undefined, { availableCount: 0, nextExpiresAt: null }]) {
      const { container, unmount } = render(<BankedResetsRow provider="claude" resetCredits={resetCredits} now={NOW} />);
      expect(container.innerHTML).toBe("");
      unmount();
    }
  });
});

describe("UseBankedReset", () => {
  it("is offered only on the live, signed-in account card that has a reset banked", () => {
    for (const props of [
      { entry: codex(), current: false },
      { entry: codex({ currentAccount: false }) },
      { entry: codex({ status: "failed" }) },
      { entry: codex({ account: null }) },
      { entry: codex({ resetCredits: { availableCount: 0, nextExpiresAt: null } }) },
      { entry: codex({ resetCredits: null }) },
    ]) {
      const { unmount } = render(button(props));
      expect(screen.queryByRole("button", { name: "Use banked reset" })).toBeNull();
      unmount();
    }

    render(button({ entry: codex() }));
    expect(useReset()).toBeTruthy();
  });

  it("stays unusable while the account controls are busy", () => {
    render(button({ entry: codex({ windows: [weekly(99)] }), disabled: true }));

    expect(useReset()).toHaveProperty("disabled", true);
    fireEvent.click(useReset());
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();
  });

  it.each([
    ["5% left", [weekly(95)], "asks"],
    ["4.5% left", [weekly(95.5)], "spends"],
    ["the fuller main window decides", [weekly(40), session(96)], "spends"],
  ] as const)("with %s it %s", async (_, windows, expected) => {
    render(button({ entry: codex({ windows: [...windows] }) }));

    fireEvent.click(useReset());

    if (expected === "asks") {
      expect(screen.getByRole("alertdialog")).toBeTruthy();
      expect(consumeCodexResetCredit).not.toHaveBeenCalled();
    } else {
      expect(screen.queryByRole("alertdialog")).toBeNull();
      await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledTimes(1));
    }
  });

  it("spends the reset on the card's account and says so", async () => {
    render(button({ entry: codex({ windows: [weekly(96), session(12)] }) }));

    fireEvent.click(useReset());

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    expect(consumeCodexResetCredit).toHaveBeenCalledWith("acct-work", expect.stringMatching(UUID));
  });

  it("asks before spending a reset while usage is still left, naming the account and how much is left", async () => {
    render(button({ entry: codex(), label: "saved@codex.example" }));

    fireEvent.click(useReset());
    const dialog = screen.getByRole("alertdialog", { name: "Use a banked reset now?" });
    expect(within(dialog).getByText(/^saved@codex\.example still has 60% of its Codex usage left/)).toBeTruthy();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();

    fireEvent.click(useReset());
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Use reset anyway" }));
    await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledWith("acct-work", expect.stringMatching(UUID)));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("asks when on-n-off cannot tell how much usage is left", () => {
    render(button({ entry: codex({ windows: [] }) }));

    fireEvent.click(useReset());

    expect(within(screen.getByRole("alertdialog")).getByText(/can't tell how much Codex usage work@codex\.example has left/)).toBeTruthy();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();
  });

  it.each(["spends straight away", "asks first"] as const)("a second click while the first spend runs does nothing (%s)", async (path) => {
    const pending = deferred<string>();
    consumeCodexResetCredit.mockReturnValue(pending.promise);
    render(button({ entry: codex({ windows: [weekly(path === "asks first" ? 40 : 99)] }) }));

    fireEvent.click(useReset());
    if (path === "asks first") fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Use reset anyway" }));
    const busy = await screen.findByRole("button", { name: "Using reset…" });
    expect(busy).toHaveProperty("disabled", true);
    fireEvent.click(busy);

    pending.resolve("reset");
    await waitFor(() => expect(screen.getByRole("button", { name: "Use banked reset" })).toHaveProperty("disabled", false));
    expect(consumeCodexResetCredit).toHaveBeenCalledTimes(1);
  });

  it("retries an attempt that failed under the same key, and starts a new key after a definite answer", async () => {
    consumeCodexResetCredit
      .mockRejectedValueOnce({ kind: "message", message: "Codex app-server timed out." })
      .mockResolvedValueOnce("reset")
      .mockResolvedValueOnce("nothingToReset");
    render(button({ entry: codex({ windows: [weekly(99)] }) }));

    fireEvent.click(useReset());
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Codex app-server timed out."));
    fireEvent.click(useReset());
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    fireEvent.click(useReset());
    await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledTimes(3));

    const [first, retry, next] = consumeCodexResetCredit.mock.calls.map((call) => call[1]);
    // The timed-out request may already have spent the reset; reusing its key lets Codex say so.
    expect(retry).toBe(first);
    expect(next).not.toBe(first);
  });

  it("keeps the result in view after the refresh takes the count to zero", async () => {
    const { rerender } = render(button({ entry: codex({ windows: [weekly(99)] }) }));

    fireEvent.click(useReset());
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    rerender(button({ entry: codex({ windows: [weekly(0)], resetCredits: { availableCount: 0, nextExpiresAt: null } }) }));

    expect(screen.getByRole("status").textContent).toBe("Banked reset used.");
    expect(screen.queryByRole("button", { name: "Use banked reset" })).toBeNull();
  });

  it.each([
    ["nothingToReset", "Nothing to reset: this account's usage is already at 0%."],
    ["noCredit", "This account has no banked reset left."],
    ["alreadyRedeemed", "That banked reset was already used."],
    ["unknown", "Codex answered with a result on-n-off doesn't recognize. Check the reset count after the refresh."],
  ])("explains a %s outcome", async (outcome, message) => {
    consumeCodexResetCredit.mockResolvedValue(outcome);
    render(button({ entry: codex({ windows: [weekly(99)] }) }));

    fireEvent.click(useReset());

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(message));
  });

  it("shows why a reset could not be spent", async () => {
    consumeCodexResetCredit.mockRejectedValue({
      kind: "message",
      message: "The signed-in Codex account changed. Try again on its card.",
    });
    render(button({ entry: codex({ windows: [weekly(99)] }) }));

    fireEvent.click(useReset());

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toBe("The signed-in Codex account changed. Try again on its card."),
    );
  });
});

describe("ResetOfferRow", () => {
  it("names the price the provider is offering and says where buying happens", () => {
    render(<ResetOfferRow offer={{ price: { currency: "USD", amountMinorUnits: 800 } }} />);
    expect(screen.getByRole("definition", { name: "Paid reset" })).toHaveTextContent("$8.00");
    expect(screen.getByText("offered by Codex · buy it on chatgpt.com")).toBeVisible();
  });

  it("still shows the offer when the provider names no price", () => {
    render(<ResetOfferRow offer={{}} />);
    expect(screen.getByRole("definition", { name: "Paid reset" })).toHaveTextContent(/^offered$/);
  });

  it("shows nothing when no reset is offered, which is most of the time", () => {
    const { container } = render(<ResetOfferRow offer={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("is shown, never sold: the row carries no way to buy anything", () => {
    render(<ResetOfferRow offer={{ price: { currency: "USD", amountMinorUnits: 800 } }} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
