import { useState } from "react";
import { confirmItem, type ActionMenuItem } from "@/components/ActionMenu";
import type { CardAccount, LimitCard } from "./limitCards";

export function removeAccountQuestion(label: string): string {
  return `Remove ${label} from on-n-off? You will need to sign in to add it again.`;
}

export type HistoryActions = {
  items: ActionMenuItem[];
  danger: ActionMenuItem[];
  archive: { run: () => void; busy: boolean };
  removing: boolean;
};

export function useHistoryActions(card: LimitCard, { blocked, onArchive, onRemove }: {
  blocked: boolean;
  onArchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
}): HistoryActions {
  const [archiving, setArchiving] = useState(false);
  const [removing, setRemoving] = useState(false);
  const { account } = card;
  const disabled = blocked || removing;
  function archive() {
    if (archiving || !account) return;
    setArchiving(true);
    void onArchive(account).finally(() => setArchiving(false));
  }
  const offered = card.active ? null : account;
  return {
    items: offered ? [{ id: "archive", label: "Archive account", disabled, busy: archiving, run: archive }] : [],
    danger: offered ? [confirmItem({
      id: "remove",
      label: "Remove account",
      question: removeAccountQuestion(offered.name),
      confirmLabel: "Confirm removal",
      disabled,
      onConfirm: async done => {
        setRemoving(true);
        try { await onRemove(offered); }
        finally { setRemoving(false); }
        done();
      },
    })] : [],
    archive: { run: archive, busy: archiving },
    removing,
  };
}
