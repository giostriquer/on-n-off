import { useRef } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SavedProfile } from "$lib/accountTypes";
import { ArchivedAccounts } from "./ArchivedAccounts";
import { limitColumn, type CardAccount, type LimitCard } from "./limitCards";
import { NOW, okCodex, staleCodex } from "./readingFixtures";

const unread: SavedProfile = {
  id: "unread", observationId: "profile:unread", identity: { provider: "codex", userId: "unread", workspaceId: "team" },
  email: "unread@codex.example", label: "unread@codex.example", category: "Client B", savedAt: NOW, active: false, needsLogin: false, archived: true,
};

/** A history card and a profile-only card, both archived, beside the signed-in account. */
function archived(): LimitCard[] {
  const column = limitColumn({
    provider: "codex",
    entries: [okCodex(), staleCodex({ account: { id: "team", label: "history@codex.example" }, archived: true })],
    profiles: [unread],
    now: Date.parse(NOW),
  });
  if (!column) throw new Error("expected a column");
  return column.archived;
}

function renderList(cards: LimitCard[], handlers: { onUnarchive?: (account: CardAccount) => Promise<void>; onRemove?: (account: CardAccount) => Promise<void> } = {}) {
  const onUnarchive = handlers.onUnarchive ?? vi.fn().mockResolvedValue(undefined);
  const onRemove = handlers.onRemove ?? vi.fn().mockResolvedValue(undefined);
  render(<ArchivedAccounts provider="codex" cards={cards} blocked={false} onUnarchive={onUnarchive} onRemove={onRemove} />);
  return { onUnarchive, onRemove };
}

describe("the archived accounts list", () => {
  it("shows nothing while no account is archived", () => {
    const { container } = render(<ArchivedAccounts provider="codex" cards={[]} blocked={false} onUnarchive={vi.fn()} onRemove={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("stays collapsed behind a disclosure that counts the archived accounts, and lists them without meters when opened", () => {
    renderList(archived());
    const disclosure = screen.getByRole("button", { name: "Archived (2)" });
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("listitem")).toBeNull();

    fireEvent.click(disclosure);

    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    const list = screen.getByRole("list", { name: "Archived Codex accounts" });
    expect(disclosure).toHaveAttribute("aria-controls", list.id);
    const [history, profileOnly] = within(list).getAllByRole("listitem");
    expect(within(history).getByText("history@codex.example")).toBeInTheDocument();
    expect(within(profileOnly).getByText("unread@codex.example")).toBeInTheDocument();
    expect(within(profileOnly).getByText("Client B")).toBeInTheDocument();
    expect(within(list).queryByRole("meter")).toBeNull();
    expect(within(list).queryByRole("button", { name: /Use account/ })).toBeNull();
  });

  it("unarchives a row's account, and names every row action after its account", async () => {
    const { onUnarchive } = renderList(archived());
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));
    expect(screen.getByRole("button", { name: "Remove account history@codex.example" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Unarchive unread@codex.example" }));

    await waitFor(() => expect(onUnarchive).toHaveBeenCalledTimes(1));
    expect(vi.mocked(onUnarchive).mock.calls[0][0]).toMatchObject({ id: "profile:unread", forget: [{ accountId: "profile:unread" }] });
  });

  it("offers nothing while the account controls are blocked", () => {
    render(<ArchivedAccounts provider="codex" cards={archived()} blocked onUnarchive={vi.fn()} onRemove={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));
    for (const label of ["history@codex.example", "unread@codex.example"]) {
      expect(screen.getByRole("button", { name: `Unarchive ${label}` })).toBeDisabled();
      expect(screen.getByRole("button", { name: `Remove account ${label}` })).toBeDisabled();
    }
  });

  it("offers a row nothing more while its unarchive is running", async () => {
    let finish!: () => void;
    const onUnarchive = vi.fn(() => new Promise<void>(resolve => { finish = resolve; }));
    renderList(archived(), { onUnarchive });
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));

    fireEvent.click(screen.getByRole("button", { name: "Unarchive history@codex.example" }));

    expect(screen.getByRole("button", { name: "Unarchive history@codex.example" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove account history@codex.example" })).toBeDisabled();
    await act(async () => { finish(); });
    expect(screen.getByRole("button", { name: "Unarchive history@codex.example" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Remove account history@codex.example" })).toBeEnabled();
  });

  it("closes the confirmation on Cancel or Escape, handing focus back to Remove account", () => {
    renderList(archived());
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));
    const remove = screen.getByRole("button", { name: "Remove account history@codex.example" });

    fireEvent.click(remove);
    fireEvent.click(within(screen.getByRole("group", { name: "Confirm account action" })).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull();
    expect(remove).toHaveFocus();

    fireEvent.click(remove);
    fireEvent.keyDown(screen.getByRole("button", { name: "Confirm removal" }), { key: "Tab" });
    expect(screen.getByRole("group", { name: "Confirm account action" }), "only Escape closes it").toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("button", { name: "Confirm removal" }), { key: "Escape" });
    expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull();
    expect(remove).toHaveFocus();
  });

  it("asks before Remove account as a card does, and says why a removal failed", async () => {
    const onRemove = vi.fn().mockRejectedValueOnce({ kind: "message", message: "disk failed", path: null }).mockResolvedValue(undefined);
    renderList(archived(), { onRemove });
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));

    fireEvent.click(screen.getByRole("button", { name: "Remove account history@codex.example" }));
    const confirm = screen.getByRole("group", { name: "Confirm account action" });
    expect(confirm).toHaveTextContent("Remove history@codex.example from on-n-off? You will need to sign in to add it again.");
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(onRemove).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Remove account history@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("disk failed");
    expect(onRemove.mock.calls[0][0]).toMatchObject({ id: "team" });

    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(onRemove).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("group", { name: "Confirm account action" })).toBeNull());
    expect(screen.queryByRole("alert"), "a removal that went through leaves no error").toBeNull();
  });
});

/** The list below a stand-in for the column's first card, which is where focus goes once no row can take it. */
function Column({ cards, onUnarchive = vi.fn().mockResolvedValue(undefined), onRemove = vi.fn().mockResolvedValue(undefined) }: {
  cards: LimitCard[]; onUnarchive?: (account: CardAccount) => Promise<void>; onRemove?: (account: CardAccount) => Promise<void>;
}) {
  const first = useRef<HTMLButtonElement>(null);
  return <><button ref={first}>First card</button>
    <ArchivedAccounts provider="codex" cards={cards} blocked={false} onUnarchive={onUnarchive} onRemove={onRemove} focusWhenEmpty={() => first.current} /></>;
}

describe("focus as an archived row leaves", () => {
  it("goes to the next row's same button, else the previous row's", async () => {
    const [history, unread] = archived();
    const onUnarchive = vi.fn().mockResolvedValue(undefined);
    const onRemove = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(<Column cards={[history, unread]} onUnarchive={onUnarchive} onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));

    screen.getByRole("button", { name: "Unarchive history@codex.example" }).focus();
    fireEvent.click(screen.getByRole("button", { name: "Unarchive history@codex.example" }));
    await waitFor(() => expect(onUnarchive).toHaveBeenCalled());
    rerender(<Column cards={[unread]} onUnarchive={onUnarchive} onRemove={onRemove} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Unarchive unread@codex.example" })).toHaveFocus());

    rerender(<Column cards={[history, unread]} onUnarchive={onUnarchive} onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove account unread@codex.example" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(onRemove).toHaveBeenCalled());
    rerender(<Column cards={[history]} onUnarchive={onUnarchive} onRemove={onRemove} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove account history@codex.example" })).toHaveFocus());
  });

  it("goes to the disclosure when no neighbouring row has the button, and to the column once the list is empty", async () => {
    const [history] = archived();
    const bare = { ...history, key: "bare", account: null };
    const { rerender } = render(<Column cards={[bare, history]} />);
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));
    screen.getByRole("button", { name: "Unarchive history@codex.example" }).focus();
    fireEvent.click(screen.getByRole("button", { name: "Unarchive history@codex.example" }));
    rerender(<Column cards={[bare]} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Archived (1)" })).toHaveFocus());

    rerender(<Column cards={[history]} />);
    screen.getByRole("button", { name: "Unarchive history@codex.example" }).focus();
    fireEvent.click(screen.getByRole("button", { name: "Unarchive history@codex.example" }));
    rerender(<Column cards={[]} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "First card" })).toHaveFocus());
  });

  it("stays where the user moved it before the row left", async () => {
    const [history, unread] = archived();
    const onUnarchive = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(<Column cards={[history, unread]} onUnarchive={onUnarchive} />);
    fireEvent.click(screen.getByRole("button", { name: "Archived (2)" }));
    fireEvent.click(screen.getByRole("button", { name: "Unarchive history@codex.example" }));
    await waitFor(() => expect(onUnarchive).toHaveBeenCalled());

    screen.getByRole("button", { name: "First card" }).focus();
    rerender(<Column cards={[unread]} onUnarchive={onUnarchive} />);
    await act(async () => {});
    expect(screen.getByRole("button", { name: "First card" })).toHaveFocus();
  });
});
