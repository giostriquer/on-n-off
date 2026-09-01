import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { NotchSnapshot } from "$lib/notchTypes";
import {
  layoutDisplays,
  NotchSettingsCard,
  showChoice,
  showPatch,
  toggleNotchList, toggleNotchProvider,
} from "./NotchSettingsCard";

const calls = vi.hoisted(() => ({ read: vi.fn(), save: vi.fn() }));
vi.mock("$lib/api", () => ({
  readNotchState: calls.read,
  saveNotchSettings: calls.save,
  onNotchChanged: async () => () => undefined,
}));
let snapshot: NotchSnapshot;
const ALL = ["claude", "codex", "antigravity", "cursor"] as const;

beforeEach(() => {
  snapshot = {
    revision: 0,
    supported: true,
    settings: {
      enabled: false,
      displayId: null,
      edge: "right",
      size: "standard",
      show: "always",
      providers: [...ALL],
      pullRequests: { enabled: true, lists: ["mine"] },
    },
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

const button = (name: string) => screen.getByRole("button", { name });

it("requires an explicit display before the rail can be shown, then saves its identity", async () => {
  mount();
  const selector = await screen.findByRole("combobox", { name: "Display" });
  await waitFor(() => expect(selector).not.toBeDisabled());
  expect(button("Always show")).toBeDisabled();
  expect(button("Show on hover")).toBeDisabled();
  expect(button("Hide")).toHaveAttribute("aria-pressed", "true");
  fireEvent.change(selector, { target: { value: "second" } });
  await waitFor(() => expect(button("Show on hover")).not.toBeDisabled());
  fireEvent.click(button("Show on hover"));
  await waitFor(() =>
    expect(button("Show on hover")).toHaveAttribute("aria-pressed", "true"),
  );
  expect(calls.save).toHaveBeenLastCalledWith({
    enabled: true,
    displayId: "second",
    edge: "right",
    size: "standard",
    show: "onHover",
    providers: [...ALL],
    pullRequests: { enabled: true, lists: ["mine"] },
  });
  expect(screen.getByText(/small pill at the screen edge/)).toBeTruthy();
});

it("keeps a disconnected display selected and lets the notch be hidden", async () => {
  snapshot.settings = { ...snapshot.settings, enabled: true, displayId: "disconnected", edge: "left" };
  mount();
  expect(
    await screen.findByRole("option", { name: /Saved display/ }),
  ).toBeTruthy();
  expect(screen.getByRole("combobox")).toHaveValue("disconnected");
  await waitFor(() => expect(button("Always show")).toHaveAttribute("aria-pressed", "true"));
  fireEvent.click(button("Hide"));
  await waitFor(() => expect(button("Hide")).toHaveAttribute("aria-pressed", "true"));
  expect(calls.save).toHaveBeenLastCalledWith(
    expect.objectContaining({ enabled: false, displayId: "disconnected" }),
  );
  expect(button("Always show")).toBeDisabled();
});

it("does not claim a failed save succeeded", async () => {
  snapshot.settings.displayId = "first";
  calls.save.mockRejectedValue({ kind: "message", message: "Disk is full" });
  mount();
  await waitFor(() => expect(button("Always show")).not.toBeDisabled());
  fireEvent.click(button("Always show"));
  expect(await screen.findByRole("alert")).toHaveTextContent("Disk is full");
  expect(button("Hide")).toHaveAttribute("aria-pressed", "true");
});

it("does not offer a mirrored display as a single-display target", async () => {
  snapshot.displays[1].mirrored = true;
  mount();
  expect(
    await screen.findByRole("option", { name: /mirrored/ }),
  ).toBeDisabled();
});

it("offers four edges and three size presets and persists the choice", async () => {
  mount();
  await screen.findByRole("group", { name: "Edge" });
  await waitFor(() =>
    expect(button("Right")).toHaveAttribute("aria-pressed", "true"),
  );
  fireEvent.click(button("Top"));
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(expect.objectContaining({ edge: "top" })),
  );
  expect(await screen.findByText(/below the menu bar/)).toBeTruthy();
  fireEvent.click(button("Large"));
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ edge: "top", size: "large" }),
    ),
  );
});

it("toggles providers in rail order and never removes the last one", async () => {
  snapshot.settings.pullRequests = { enabled: false, lists: ["mine"] };
  snapshot.settings.providers = ["claude", "cursor"];
  mount();
  const cursor = await screen.findByRole("switch", { name: "Show Cursor in the notch" });
  await waitFor(() => expect(cursor).not.toBeDisabled());
  expect(cursor).toHaveAttribute("aria-checked", "true");
  expect(screen.getByRole("switch", { name: "Show Codex in the notch" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  fireEvent.click(screen.getByRole("switch", { name: "Show Codex in the notch" }));
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ providers: ["claude", "codex", "cursor"] }),
    ),
  );
  fireEvent.click(cursor);
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ providers: ["claude", "codex"] }),
    ),
  );
  fireEvent.click(screen.getByRole("switch", { name: "Show Codex in the notch" }));
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ providers: ["claude"] }),
    ),
  );
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Show Claude in the notch" })).toBeDisabled(),
  );
});

it("maps the show control onto enabled + show and refuses to drop the last provider", () => {
  expect(showChoice({ enabled: false, show: "onHover" })).toBe("hide");
  expect(showChoice({ enabled: true, show: "onHover" })).toBe("hover");
  expect(showChoice({ enabled: true, show: "always" })).toBe("always");
  expect(showPatch("hide")).toEqual({ enabled: false });
  expect(showPatch("hover")).toEqual({ enabled: true, show: "onHover" });
  expect(toggleNotchProvider(["codex"], "claude", true)).toEqual(["claude", "codex"]);
  expect(toggleNotchProvider(["codex"], "codex", false)).toEqual(["codex"]);
});

it("lays monitors out by physical coordinates instead of API order", () => {
  const displays = [
    { ...snapshot.displays[0], id: "right", x: 1920 },
    { ...snapshot.displays[0], id: "left", x: -1920 },
    { ...snapshot.displays[0], id: "middle", x: 0 },
  ];

  const layout = layoutDisplays(displays);

  expect(layout.map(({ id }) => id)).toEqual(["left", "middle", "right"]);
  expect(layout.find(({ id }) => id === "left")?.left).toBe(0);
  expect(layout.find(({ id }) => id === "middle")?.left).toBeCloseTo(100 / 3);
  expect(layout.find(({ id }) => id === "right")?.left).toBeCloseTo(200 / 3);
  expect(layout.find(({ id }) => id === "middle")?.order).toBe(2);
});

it("shows only the user's own pull requests by default and lets other lists join in order", async () => {
  mount();
  const toggle = await screen.findByRole("switch", { name: "Show pull requests in the notch" });
  await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
  const lists = screen.getByRole("group", { name: "Pull request lists" });
  expect(lists).toBeTruthy();
  expect(screen.getByRole("button", { name: "Mine" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Mine" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Assigned" }));
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ pullRequests: { enabled: true, lists: ["mine", "assigned"] } }),
    ),
  );
  fireEvent.click(toggle);
  await waitFor(() =>
    expect(calls.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ pullRequests: { enabled: false, lists: ["mine", "assigned"] } }),
    ),
  );
  expect(toggleNotchList(["assigned"], "assigned", false)).toEqual(["assigned"]);
  expect(toggleNotchList(["assigned"], "reviewRequested", true)).toEqual(["reviewRequested", "assigned"]);
});
