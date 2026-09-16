import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderLimits } from "$lib/limitsTypes";
import { CodexAccountActions } from "./CodexAccountActions";

const api = vi.hoisted(() => ({
  consumeCodexResetCredit: vi.fn(),
  connectCodexBilling: vi.fn(),
  readCodexSubscription: vi.fn(),
  onSharedReadChanged: vi.fn(() => Promise.resolve(() => undefined)),
}));
vi.mock("$lib/api", () => api);

const NOW = Date.parse("2026-08-17T20:00:00Z");
const entry: ProviderLimits = {
  provider: "codex",
  status: "ok",
  currentAccount: true,
  account: { id: "acct-work", label: "work@codex.example" },
  windows: [{ id: "secondary", label: "Weekly · all models", kind: "weekly", usedPercent: 40, resetsAt: "2026-08-22T00:00:00Z", observedAt: "2026-08-17T20:00:00Z" }],
  resetCredits: { availableCount: 1, nextExpiresAt: null },
};

function renderActions(state: { current: boolean; blocked: boolean; unconfirmedCurrent: boolean }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CodexAccountActions entry={entry} label="work@codex.example" now={NOW} state={state} />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  api.readCodexSubscription.mockReset().mockResolvedValue({ metadata: null, connected: false, unavailable: false, browserSupported: true, canConnect: true });
});

describe("CodexAccountActions", () => {
  it("offers the banked reset and billing on a Codex card", async () => {
    renderActions({ current: true, blocked: false, unconfirmedCurrent: false });

    expect(screen.getByRole("button", { name: "Use banked reset" })).toHaveProperty("disabled", false);
    expect(await screen.findByRole("button", { name: "Connect billing" })).toHaveProperty("disabled", false);
  });

  it("holds the banked reset until the native login is confirmed, without holding billing", async () => {
    renderActions({ current: true, blocked: false, unconfirmedCurrent: true });

    expect(screen.getByRole("button", { name: "Use banked reset" })).toHaveProperty("disabled", true);
    expect(await screen.findByRole("button", { name: "Connect billing" })).toHaveProperty("disabled", false);
  });

  it("blocks both while the account controls cannot act", async () => {
    renderActions({ current: true, blocked: true, unconfirmedCurrent: false });

    expect(screen.getByRole("button", { name: "Use banked reset" })).toHaveProperty("disabled", true);
    await waitFor(() => expect(screen.getByRole("button", { name: "Connect billing" })).toHaveProperty("disabled", true));
  });
});
