import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId, ResetAlert } from "$lib/types";
import { Limits } from "./Limits";
import { okClaude, okCodex } from "./readingFixtures";

const readAccounts = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const requestNotificationPermission = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, requestNotificationPermission,
  accountAction: vi.fn(), forgetLimitsSnapshot: vi.fn(), setLimitsArchived: vi.fn(),
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  addAccount: vi.fn(), cancelAccountLogin: vi.fn(), consumeCodexResetCredit: vi.fn(), pendingResetSpends: vi.fn().mockResolvedValue([]), cancelResetSpend: vi.fn(),
  onSharedReadChanged: () => Promise.resolve(() => {}), readCodexSubscription: vi.fn().mockResolvedValue(null),
}));

function answer(claude: ProviderLimits[], codex: ProviderLimits[]) {
  readLimits.mockImplementation((agentId: AgentId) => Promise.resolve(agentId === "claude" ? claude : codex));
}

function renderLimits(resetAlerts: Record<string, ResetAlert> = {}) {
  const onResetAlertsChange = vi.fn().mockResolvedValue(undefined);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  render(
    <QueryClientProvider client={client}>
      <Limits resetAlerts={resetAlerts} onResetAlertsChange={onResetAlertsChange} />
    </QueryClientProvider>,
  );
  return onResetAlertsChange;
}

async function openAlert(label: string) {
  fireEvent.click(await screen.findByRole("button", { name: `More actions for ${label}` }));
  fireEvent.click(within(screen.getByRole("group", { name: `Actions for ${label}` })).getByRole("button", { name: "Banked reset alert" }));
  return screen.getByRole("group", { name: "Banked reset alert" });
}

const ALERT = "Tell me when this account's banked reset is worth using";

beforeEach(() => {
  readAccounts.mockReset().mockResolvedValue({ profiles: [], nativeAccount: null, recoveryRequired: false, notice: null });
  requestNotificationPermission.mockReset().mockResolvedValue(true);
  answer([okClaude()], [okCodex()]);
});

describe("a Codex account's banked reset alert", () => {
  it("is turned on from the card's menu with Codex's own share and a day's wait", async () => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull();
    expect(within(form).getByLabelText("With this much of the limit left or less (%)")).toHaveProperty("value", "10");
    expect(within(form).getByLabelText("And at least this many hours before it renews by itself")).toHaveProperty("value", "24");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24, automatic: false },
    }));
    expect(requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("group", { name: "Banked reset alert" })).toBeNull();
  });

  it("takes focus into its form when opened, and Escape gives it back to the menu button", async () => {
    renderLimits();

    const form = await openAlert("work@codex.example");

    expect(document.activeElement).toBe(within(form).getByRole("button", { name: ALERT }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "More actions for work@codex.example" }));
    expect(screen.queryByRole("group", { name: "Banked reset alert" })).toBeNull();
  });

  it("can use the reset by itself, ten minutes after saying so", async () => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    expect(within(form).getByRole("button", { name: "Notify me" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(within(form).getByRole("button", { name: "Use it" }));
    expect(within(form).getByRole("button", { name: "Use it" })).toHaveAttribute("aria-pressed", "true");
    expect(within(form).getByRole("button", { name: "Notify me" })).toHaveAttribute("aria-pressed", "false");
    expect(form).toHaveTextContent("on-n-off tells you, waits 10 minutes, then uses the reset unless you cancel it on this card.");
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24, automatic: true },
    }));
  });

  it("opens an alert that uses the reset by itself as such, and can be turned back to notifying", async () => {
    const saved = renderLimits({ "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24, automatic: true } });

    const form = await openAlert("work@codex.example");
    expect(within(form).getByRole("button", { name: "Use it" })).toHaveAttribute("aria-pressed", "true");
    expect(form).toHaveTextContent("waits 10 minutes");
    fireEvent.click(within(form).getByRole("button", { name: "Notify me" }));
    expect(within(form).getByRole("button", { name: "Notify me" })).toHaveAttribute("aria-pressed", "true");
    expect(within(form).getByRole("button", { name: "Use it" })).toHaveAttribute("aria-pressed", "false");
    expect(form).toHaveTextContent("You use the reset from this card");
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24, automatic: false },
    }));
  });

  it("keeps a lower share and a longer wait", async () => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.change(within(form).getByLabelText("With this much of the limit left or less (%)"), { target: { value: "5" } });
    fireEvent.change(within(form).getByLabelText("And at least this many hours before it renews by itself"), { target: { value: "48" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: 5, minHoursToRenewal: 48, automatic: false },
    }));
  });

  it.each([["1", "0"], ["10", "168"]])("keeps %s percent left and %s hours, the ends of what it allows", async (left, hours) => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.change(within(form).getByLabelText("With this much of the limit left or less (%)"), { target: { value: left } });
    fireEvent.change(within(form).getByLabelText("And at least this many hours before it renews by itself"), { target: { value: hours } });
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: Number(left), minHoursToRenewal: Number(hours), automatic: false },
    }));
  });

  it.each([["11", "24"], ["0", "24"], ["10", "169"], ["2.5", "24"], ["5", "-1"], ["5", "24.5"]])("refuses %s percent left and %s hours", async (left, hours) => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.change(within(form).getByLabelText("With this much of the limit left or less (%)"), { target: { value: left } });
    fireEvent.change(within(form).getByLabelText("And at least this many hours before it renews by itself"), { target: { value: hours } });

    expect(within(form).getByRole("button", { name: "Save alert" })).toHaveProperty("disabled", true);
    expect(within(form).getByRole("alert").textContent).toBe("Use 1 to 10% left and 0 to 168 hours.");
    expect(saved).not.toHaveBeenCalled();
  });

  it("is not turned on when notifications are blocked, since it would never be seen, and can be tried again", async () => {
    requestNotificationPermission.mockResolvedValue(false);
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(within(form).getByRole("alert").textContent).toBe("Notifications are blocked in system settings."));
    expect(saved).not.toHaveBeenCalled();
    expect(within(form).getByRole("button", { name: "Save alert" })).toBeEnabled();

    requestNotificationPermission.mockResolvedValue(true);
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));
    await waitFor(() => expect(saved).toHaveBeenCalledTimes(1));
  });

  it("offers its figures only while it is on", async () => {
    renderLimits();

    const form = await openAlert("work@codex.example");
    const share = within(form).getByLabelText("With this much of the limit left or less (%)");
    const wait = within(form).getByLabelText("And at least this many hours before it renews by itself");
    expect(share).toBeDisabled();
    expect(wait).toBeDisabled();

    fireEvent.click(within(form).getByRole("button", { name: ALERT }));

    expect(share).toBeEnabled();
    expect(wait).toBeEnabled();
  });

  it("keeps every other account's alert when one is turned on or off", async () => {
    const other: ResetAlert = { label: "side@codex.example", maxLeftPercent: 3, minHoursToRenewal: 12, automatic: false };
    const saved = renderLimits({ "acct-side": other });

    let form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));
    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-side": other,
      "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24, automatic: false },
    }));

    cleanup();
    const again = renderLimits({ "acct-side": other, "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24, automatic: false } });
    form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));
    await waitFor(() => expect(again).toHaveBeenCalledWith({ "acct-side": other }));
  });

  it("shows the alerts it is given once they change", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
    const tree = (alerts: Record<string, ResetAlert>) => (
      <QueryClientProvider client={client}>
        <Limits resetAlerts={alerts} onResetAlertsChange={async () => undefined} />
      </QueryClientProvider>
    );
    const { rerender } = render(tree({}));
    await screen.findByRole("button", { name: "More actions for work@codex.example" });

    rerender(tree({ "acct-work": { label: "work@codex.example", maxLeftPercent: 4, minHoursToRenewal: 36, automatic: false } }));
    const form = await openAlert("work@codex.example");

    expect(within(form).getByRole("button", { name: ALERT })).toHaveAttribute("aria-pressed", "true");
    expect(within(form).getByLabelText("With this much of the limit left or less (%)")).toHaveProperty("value", "4");
  });

  it("stays open and says so when notification permission could not be asked for", async () => {
    requestNotificationPermission.mockRejectedValue(new Error("no permission plugin"));
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(within(form).getByRole("alert").textContent).toBe("Could not request notification permission."));
    expect(saved).not.toHaveBeenCalled();
  });

  it("stays open and says so when the alert could not be saved", async () => {
    const saved = renderLimits();
    saved.mockRejectedValue(new Error("disk full"));

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(within(form).getByRole("alert").textContent).toBe("Could not save the alert."));
    expect(screen.getByRole("group", { name: "Banked reset alert" })).toBe(form);
  });

  it("is turned off by clearing it, which asks for nothing", async () => {
    const saved = renderLimits({ "acct-work": { label: "work@codex.example", maxLeftPercent: 5, minHoursToRenewal: 48, automatic: false } });

    const form = await openAlert("work@codex.example");
    expect(within(form).getByRole("button", { name: ALERT })).toHaveAttribute("aria-pressed", "true");
    expect(within(form).getByLabelText("With this much of the limit left or less (%)")).toHaveProperty("value", "5");
    fireEvent.click(within(form).getByRole("button", { name: ALERT }));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({}));
    expect(requestNotificationPermission).not.toHaveBeenCalled();
  });

  it("is offered only on Codex cards", async () => {
    renderLimits();

    fireEvent.click(await screen.findByRole("button", { name: "More actions for me@claude.example" }));

    expect(within(screen.getByRole("group", { name: "Actions for me@claude.example" })).queryByRole("button", { name: "Banked reset alert" })).toBeNull();
  });
});
