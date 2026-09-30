import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import type { AgentId, ResetAlert } from "$lib/types";
import { Limits } from "./Limits";
import { okClaude, okCodex } from "./readingFixtures";

// A Codex account's banked reset alert, turned on, changed and off from its card's menu.

const readAccounts = vi.hoisted(() => vi.fn());
const readLimits = vi.hoisted(() => vi.fn());
const requestNotificationPermission = vi.hoisted(() => vi.fn());

vi.mock("$lib/api", () => ({
  readLimits, readAccounts, requestNotificationPermission,
  accountAction: vi.fn(), forgetLimitsSnapshot: vi.fn(), setLimitsArchived: vi.fn(),
  readAccountPreferences: vi.fn().mockResolvedValue(false), readAccountActivationBlockers: vi.fn().mockResolvedValue([]),
  addAccount: vi.fn(), cancelAccountLogin: vi.fn(), consumeCodexResetCredit: vi.fn(),
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

beforeEach(() => {
  readAccounts.mockReset().mockResolvedValue({ profiles: [], nativeAccount: null, recoveryRequired: false, notice: null });
  requestNotificationPermission.mockReset().mockResolvedValue(true);
  answer([okClaude()], [okCodex()]);
});

describe("a Codex account's banked reset alert", () => {
  it("is turned on from the card's menu with Codex's own share and a day's wait", async () => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    expect(within(form).getByLabelText("With this much of the limit left or less (%)")).toHaveProperty("value", "10");
    expect(within(form).getByLabelText("And at least this many hours before it renews by itself")).toHaveProperty("value", "24");
    fireEvent.click(within(form).getByRole("checkbox"));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: 10, minHoursToRenewal: 24 },
    }));
    expect(requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("group", { name: "Banked reset alert" })).toBeNull();
  });

  it("keeps a lower share and a longer wait", async () => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("checkbox"));
    fireEvent.change(within(form).getByLabelText("With this much of the limit left or less (%)"), { target: { value: "5" } });
    fireEvent.change(within(form).getByLabelText("And at least this many hours before it renews by itself"), { target: { value: "48" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(saved).toHaveBeenCalledWith({
      "acct-work": { label: "work@codex.example", maxLeftPercent: 5, minHoursToRenewal: 48 },
    }));
  });

  it.each([["11", "24"], ["0", "24"], ["10", "169"], ["2.5", "24"]])("refuses %s%% left and %s hours", async (left, hours) => {
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("checkbox"));
    fireEvent.change(within(form).getByLabelText("With this much of the limit left or less (%)"), { target: { value: left } });
    fireEvent.change(within(form).getByLabelText("And at least this many hours before it renews by itself"), { target: { value: hours } });

    expect(within(form).getByRole("button", { name: "Save alert" })).toHaveProperty("disabled", true);
    expect(within(form).getByRole("alert").textContent).toBe("Use 1 to 10% left and 0 to 168 hours.");
    expect(saved).not.toHaveBeenCalled();
  });

  it("is not turned on when notifications are blocked, since it would never be seen", async () => {
    requestNotificationPermission.mockResolvedValue(false);
    const saved = renderLimits();

    const form = await openAlert("work@codex.example");
    fireEvent.click(within(form).getByRole("checkbox"));
    fireEvent.click(within(form).getByRole("button", { name: "Save alert" }));

    await waitFor(() => expect(within(form).getByRole("alert").textContent).toBe("Notifications are blocked in system settings."));
    expect(saved).not.toHaveBeenCalled();
  });

  it("is turned off by clearing it, which asks for nothing", async () => {
    const saved = renderLimits({ "acct-work": { label: "work@codex.example", maxLeftPercent: 5, minHoursToRenewal: 48 } });

    const form = await openAlert("work@codex.example");
    expect(within(form).getByRole("checkbox")).toHaveProperty("checked", true);
    expect(within(form).getByLabelText("With this much of the limit left or less (%)")).toHaveProperty("value", "5");
    fireEvent.click(within(form).getByRole("checkbox"));
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
