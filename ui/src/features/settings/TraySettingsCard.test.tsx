import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { TraySettingsCard } from "./TraySettingsCard";

const calls = vi.hoisted(() => ({ traySupported: vi.fn() }));
vi.mock("$lib/api", () => ({ traySupported: calls.traySupported }));

const TOGGLE = "Keep on-n-off running in the tray when the window is closed";

beforeEach(() => {
  calls.traySupported.mockReset();
  calls.traySupported.mockResolvedValue(true);
});

function renderCard(closeToTray: boolean) {
  const onCloseToTrayChange = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <TraySettingsCard closeToTray={closeToTray} onCloseToTrayChange={onCloseToTrayChange} />
    </QueryClientProvider>,
  );
  return onCloseToTrayChange;
}

it("stays hidden where the platform has no tray to hide into", async () => {
  calls.traySupported.mockResolvedValue(false);
  renderCard(false);

  // Absent on the first paint, and still absent once the answer has arrived — a card headed
  // "Windows tray" offering a setting macOS ignores would be worse than no card.
  expect(screen.queryByRole("region", { name: "Windows tray" })).toBeNull();
  await waitFor(() => expect(calls.traySupported).toHaveBeenCalled());
  expect(screen.queryByRole("region", { name: "Windows tray" })).toBeNull();
});

it("shows the switch off and reports turning it on", async () => {
  const user = userEvent.setup();
  const onCloseToTrayChange = renderCard(false);

  const toggle = await screen.findByRole("button", { name: TOGGLE });
  expect(toggle).toHaveAttribute("aria-pressed", "false");

  await user.click(toggle);
  expect(onCloseToTrayChange).toHaveBeenCalledWith(true);
});

it("shows the switch on and reports turning it off", async () => {
  const user = userEvent.setup();
  const onCloseToTrayChange = renderCard(true);

  const toggle = await screen.findByRole("button", { name: TOGGLE });
  expect(toggle).toHaveAttribute("aria-pressed", "true");

  await user.click(toggle);
  expect(onCloseToTrayChange).toHaveBeenCalledWith(false);
});
