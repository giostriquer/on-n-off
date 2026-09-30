import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatShortDate } from "$lib/limitsFormat";
import type { LimitsBankedReset, LimitWindow, ProviderLimits } from "$lib/limitsTypes";
import { BankedResetsRow, ResetOfferRow, UseBankedReset } from "./BankedResets";
import { ResetAlertsContext } from "./resetAlerts";

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
  const banked = (availableCount: number, nextExpiresAt: string | null) => ({ availableCount, nextExpiresAt });

  it("reads as one more row: the count, and when the next banked reset expires", () => {
    render(<BankedResetsRow resetCredits={banked(2, "2026-08-29T15:00:00Z")} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(screen.getByText(`next expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`)).toBeTruthy();
  });

  it("says a lone reset expires, not the next one", () => {
    render(<BankedResetsRow resetCredits={banked(1, "2026-08-29T15:00:00Z")} now={NOW} />);

    expect(screen.getByText(`expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`)).toBeTruthy();
  });

  it("drops the note when the expiry is unknown", () => {
    render(<BankedResetsRow resetCredits={banked(1, null)} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("1");
    expect(screen.queryByText(/expires/)).toBeNull();
  });

  function listed(): string[] {
    const list = screen.getByRole("list", { name: "Each banked reset" });
    return within(list).getAllByRole("listitem").map(item => item.textContent ?? "");
  }

  it("lists each banked reset, soonest first, with its name and when it expires, when there is more than one", () => {
    const resetCredits = {
      availableCount: 2,
      nextExpiresAt: "2026-08-29T15:00:00Z",
      resets: [
        { title: "Full reset", expiresAt: "2026-08-29T15:00:00Z" },
        { expiresAt: "2026-09-10T00:00:00Z" },
      ] satisfies LimitsBankedReset[],
    };

    render(<BankedResetsRow resetCredits={resetCredits} now={NOW} />);

    expect(screen.getByRole("definition", { name: "Banked resets" }).textContent).toBe("2");
    expect(listed()).toEqual([
      `Full reset · expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`,
      `expires in 23d 4h · ${formatShortDate("2026-09-10T00:00:00Z")}`,
    ]);
    // The first line already says when the next one expires.
    expect(screen.queryByText(/next expires/)).toBeNull();
  });

  it("still lists a reset Codex gives neither a name nor an expiry", () => {
    const resetCredits = { availableCount: 2, nextExpiresAt: null, resets: [{}, { title: "Full reset" }] satisfies LimitsBankedReset[] };

    render(<BankedResetsRow resetCredits={resetCredits} now={NOW} />);

    expect(listed()).toEqual(["Banked reset", "Full reset"]);
  });

  it("keeps a lone banked reset's row as it was", () => {
    const resetCredits = {
      availableCount: 1,
      nextExpiresAt: "2026-08-29T15:00:00Z",
      resets: [{ title: "Full reset", expiresAt: "2026-08-29T15:00:00Z" }],
    };

    render(<BankedResetsRow resetCredits={resetCredits} now={NOW} />);

    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.getByText(`expires in 11d 19h · ${formatShortDate("2026-08-29T15:00:00Z")}`)).toBeTruthy();
  });

  // Which counts are worth a row (none, or one past its soonest expiry) is `limitCards.test.ts`'s.
  it("stays out of the card when the card has no banked resets to show", () => {
    const { container } = render(<BankedResetsRow resetCredits={null} now={NOW} />);
    expect(container.innerHTML).toBe("");
  });
});

describe("UseBankedReset", () => {
  /** Uses the reset as a person does: the button, then "Use reset" in the confirmation. */
  function spendNow() {
    fireEvent.click(useReset());
    fireEvent.click(within(screen.getByRole("alertdialog", { name: "Use this reset?" })).getByRole("button", { name: "Use reset" }));
  }

  it("is not offered once the banked reset has expired", () => {
    const { container } = render(button({ entry: codex({ resetCredits: { availableCount: 1, nextExpiresAt: "2026-08-17T19:59:00Z" } }) }));

    expect(container.innerHTML).toBe("");
  });

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

  /** Codex's own rule: a reset is used only with 10% or less of the current limit left. */
  it.each([
    ["60% left", [weekly(40), session(12)], false],
    ["11% left", [weekly(89)], false],
    ["exactly 10% left", [weekly(90)], true],
    ["the fuller main window deciding", [weekly(40), session(96)], true],
    ["no telling how much is left", [], false],
  ] as const)("with %s the reset is usable: %s", (_, windows, usable) => {
    render(button({ entry: codex({ windows: [...windows] }) }));

    expect(useReset()).toHaveProperty("disabled", !usable);
    if (usable) {
      expect(useReset().getAttribute("aria-describedby")).toBeNull();
      expect(screen.queryByText(/Usable once/)).toBeNull();
    } else {
      expect(useReset()).toHaveAccessibleDescription("Usable once 10% or less of the limit is left");
      fireEvent.click(useReset());
      expect(screen.queryByRole("alertdialog")).toBeNull();
    }
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();
  });

  it("keeps the lower share an account's alert names", () => {
    const alerts = { alerts: { "acct-work": { label: null, maxLeftPercent: 5, minHoursToRenewal: 24 } }, save: async () => undefined };
    const { rerender } = render(<ResetAlertsContext.Provider value={alerts}>{button({ entry: codex({ windows: [weekly(92)] }) })}</ResetAlertsContext.Provider>);

    expect(useReset()).toHaveProperty("disabled", true);
    expect(useReset()).toHaveAccessibleDescription("Usable once 5% or less of the limit is left");

    rerender(<ResetAlertsContext.Provider value={alerts}>{button({ entry: codex({ windows: [weekly(96)] }) })}</ResetAlertsContext.Provider>);
    expect(useReset()).toHaveProperty("disabled", false);
  });

  it("asks every time, naming the account, what is left and when the limit renews by itself", async () => {
    // The five-hour window comes first, so the renewal read is the weekly window's, not the first one's.
    render(button({ entry: codex({ windows: [session(12), weekly(96)] }), label: "saved@codex.example" }));

    fireEvent.click(useReset());
    const dialog = screen.getByRole("alertdialog", { name: "Use this reset?" });
    expect(within(dialog).getByText(/^saved@codex\.example has 4% of its Codex limit left and renews by itself in 4d 4h\./)).toBeTruthy();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(consumeCodexResetCredit).not.toHaveBeenCalled();

    spendNow();
    await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledWith("acct-work", expect.stringMatching(UUID)));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("spends the reset on the card's account and says so", async () => {
    render(button({ entry: codex({ windows: [weekly(96), session(12)] }) }));

    spendNow();

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    expect(consumeCodexResetCredit).toHaveBeenCalledWith("acct-work", expect.stringMatching(UUID));
  });

  it("a second click while the first spend runs does nothing", async () => {
    const pending = deferred<string>();
    consumeCodexResetCredit.mockReturnValue(pending.promise);
    render(button({ entry: codex({ windows: [weekly(99)] }) }));

    spendNow();
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

    spendNow();
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Codex app-server timed out."));
    spendNow();
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    spendNow();
    await waitFor(() => expect(consumeCodexResetCredit).toHaveBeenCalledTimes(3));

    const [first, retry, next] = consumeCodexResetCredit.mock.calls.map((call) => call[1]);
    // The timed-out request may already have spent the reset; reusing its key lets Codex say so.
    expect(retry).toBe(first);
    expect(next).not.toBe(first);
  });

  it("keeps the result in view after the refresh takes the count to zero", async () => {
    const { rerender } = render(button({ entry: codex({ windows: [weekly(99)] }) }));

    spendNow();
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    rerender(button({ entry: codex({ windows: [weekly(0)], resetCredits: { availableCount: 0, nextExpiresAt: null } }) }));

    expect(screen.getByRole("status").textContent).toBe("Banked reset used.");
    expect(screen.queryByRole("button", { name: "Use banked reset" })).toBeNull();
  });

  /**
   * The backend refreshes the card while the attempt is in flight, so that reading is newer than the
   * click and than the screen's `now`, and older than the answer: it must not take the message away.
   */
  it("keeps the result through the refresh the backend reads while the attempt is in flight", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(Date.parse("2026-08-17T20:00:30Z"));
      const pending = deferred<string>();
      consumeCodexResetCredit.mockReturnValue(pending.promise);
      const { rerender } = render(button({ entry: codex({ windows: [weekly(99)] }) }));
      spendNow();
      const refreshed = { ...weekly(0), observedAt: "2026-08-17T20:00:31Z" };
      rerender(button({ entry: codex({ windows: [refreshed], resetCredits: { availableCount: 0, nextExpiresAt: null } }) }));

      vi.setSystemTime(Date.parse("2026-08-17T20:00:32Z"));
      pending.resolve("reset");

      await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps what came of an attempt on a card whose windows are no longer read", async () => {
    const { rerender } = render(button({ entry: codex({ windows: [weekly(99)] }) }));
    spendNow();
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));

    rerender(button({ entry: codex({ windows: [] }) }));

    expect(screen.getByRole("status").textContent).toBe("Banked reset used.");
  });

  /** A window read `seconds` from now: a reading made after the attempt's answer came back. */
  function readLater(usedPercent: number, seconds = 60): LimitWindow {
    return { ...weekly(usedPercent), observedAt: new Date(Date.now() + seconds * 1000).toISOString() };
  }

  it("lets the result go once a later reading replaces the one the attempt left", async () => {
    const { rerender } = render(button({ entry: codex({ windows: [weekly(99)] }) }));
    spendNow();
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Banked reset used."));

    // A reset granted since: the card offers it, and says nothing of the one already spent.
    rerender(button({ entry: codex({ windows: [readLater(92)], resetCredits: { availableCount: 1, nextExpiresAt: "2026-08-29T15:00:00Z" } }) }));

    expect(screen.queryByRole("status")).toBeNull();
    expect(useReset()).toBeTruthy();
  });

  it("lets a failure go once a later reading replaces the one it was about", async () => {
    consumeCodexResetCredit.mockRejectedValue({ kind: "message", message: "Codex app-server timed out." });
    const { rerender } = render(button({ entry: codex({ windows: [weekly(99)] }) }));
    spendNow();
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Codex app-server timed out."));

    rerender(button({ entry: codex({ windows: [readLater(99)] }) }));

    expect(screen.queryByRole("alert")).toBeNull();
  });

  it.each([
    ["nothingToReset", "Nothing to reset: this account's usage is already at 0%."],
    ["noCredit", "This account has no banked reset left."],
    ["alreadyRedeemed", "That banked reset was already used."],
    ["unknown", "Codex answered with a result on-n-off doesn't recognize. Check the reset count after the refresh."],
  ])("explains a %s outcome", async (outcome, message) => {
    consumeCodexResetCredit.mockResolvedValue(outcome);
    render(button({ entry: codex({ windows: [weekly(99)] }) }));

    spendNow();

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(message));
  });

  it("shows why a reset could not be spent", async () => {
    consumeCodexResetCredit.mockRejectedValue({
      kind: "message",
      message: "A banked reset can be used once 10% or less of the current limit is left, and 15% is left.",
    });
    render(button({ entry: codex({ windows: [weekly(99)] }) }));

    spendNow();

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toBe("A banked reset can be used once 10% or less of the current limit is left, and 15% is left."),
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
