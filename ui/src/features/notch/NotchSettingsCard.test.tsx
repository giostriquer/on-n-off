import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { NotchSnapshot } from "$lib/notchTypes";
import { NotchSettingsCard } from "./NotchSettingsCard";

const calls = vi.hoisted(() => ({ read: vi.fn(), save: vi.fn() }));
vi.mock("$lib/api", () => ({
  readNotchState: calls.read,
  saveNotchSettings: calls.save,
  onNotchChanged: async () => () => undefined,
}));
let snapshot: NotchSnapshot;

beforeEach(() => {
  snapshot = {
    revision: 0,
    supported: true,
    settings: { enabled: false, displayId: null, edge: "right", size: "standard" },
    error: null,
    displays: ["first", "second"].map((id, index) => ({
      id,
      name: "Identical name",
      x: index * 1920,
      y: 0,
      width: 1920,
      height: 1080,
      workY: 24,
      workHeight: 1056,
      scale: 1,
      mirrored: false,
    })),
  };
  calls.read.mockReset().mockImplementation(async () => snapshot);
  calls.save.mockReset().mockImplementation(async (settings) => {
    snapshot = { ...snapshot, settings };
    return snapshot;
  });
});

function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <NotchSettingsCard />
    </QueryClientProvider>,
  );
}

it("requires an explicit display and saves its identity even when names match", async () => {
  mount();
  const selector = await screen.findByRole("combobox", { name: "Display" });
  expect(screen.getByRole("switch")).toBeDisabled();
  await waitFor(() => expect(selector).not.toBeDisabled());
  fireEvent.change(selector, { target: { value: "second" } });
  await waitFor(() => expect(screen.getByRole("switch")).not.toBeDisabled());
  fireEvent.click(screen.getByRole("switch"));
  await waitFor(() =>
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true"),
  );
  expect(calls.save).toHaveBeenLastCalledWith({
    enabled: true,
    displayId: "second",
    edge: "right",
    size: "standard",
  });
});

it("keeps a disconnected display selected and allows the notch to be disabled", async () => {
  snapshot.settings = {
    enabled: true,
    displayId: "disconnected",
    edge: "left",
    size: "standard",
  };
  mount();
  expect(
    await screen.findByRole("option", { name: /Saved display/ }),
  ).toBeTruthy();
  expect(screen.getByRole("combobox")).toHaveValue("disconnected");
  fireEvent.click(screen.getByRole("switch"));
  await waitFor(() =>
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false"),
  );
  expect(screen.getByRole("switch")).toBeDisabled();
});

it("does not claim a failed save succeeded", async () => {
  snapshot.settings.displayId = "first";
  calls.save.mockRejectedValue({ kind: "message", message: "Disk is full" });
  mount();
  await waitFor(() => expect(screen.getByRole("switch")).not.toBeDisabled());
  fireEvent.click(screen.getByRole("switch"));
  expect(await screen.findByRole("alert")).toHaveTextContent("Disk is full");
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
});

it("does not offer a mirrored display as a single-display target", async () => {
  snapshot.displays[1].mirrored = true;
  mount();
  expect(
    await screen.findByRole("option", { name: /mirrored/ }),
  ).toBeDisabled();
});

it("offers three size presets and persists the selected preset", async () => {
  mount();
  const sizes = await screen.findByRole("group", { name: "Size" });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Standard" })).toHaveAttribute(
      "aria-pressed",
      "true",
    ),
  );

  fireEvent.click(screen.getByRole("button", { name: "Large" }));

  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith({
      enabled: false,
      displayId: null,
      edge: "right",
      size: "large",
    }),
  );
  expect(sizes).toBeTruthy();
});
