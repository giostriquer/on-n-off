import { useState } from "react";
import { confirmItem, type ActionMenuItem } from "@/components/ActionMenu";
import { actionButton as button } from "@/components/actionButton";
import { parseInvokeError } from "$lib/error";
import type { CardAccount, LimitCard } from "./limitCards";

export function removeAccountQuestion(label: string): string {
  return `Remove ${label} from on-n-off? You will need to sign in to add it again.`;
}

export type CardActions = {
  category: ActionMenuItem[];
  items: ActionMenuItem[];
  danger: ActionMenuItem[];
  archive: { run: () => void; busy: boolean };
  removing: boolean;
};

export function useCardActions(card: LimitCard, { blocked, onArchive, onRemove, onCategory }: {
  blocked: boolean;
  onArchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
  onCategory: (account: CardAccount, category: string) => Promise<void>;
}): CardActions {
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
    category: account ? [{
      id: "category",
      label: "Edit category",
      disabled,
      panel: done => <CategoryPanel initial={card.identity.category ?? ""} disabled={disabled} onSave={category => onCategory(account, category)} done={done} />,
    }] : [],
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

function CategoryPanel({ initial, disabled, onSave, done }: { initial: string; disabled: boolean; onSave: (category: string) => Promise<void>; done: () => void }) {
  const [category, setCategory] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function save() {
    setError(null);
    setSaving(true);
    try {
      await onSave(category);
      done();
    } catch (reason) {
      setError(parseInvokeError(reason).message);
    } finally {
      setSaving(false);
    }
  }
  return <>
    <form role="group" aria-label="Edit account category" className="flex flex-wrap gap-2" onSubmit={event => { event.preventDefault(); void save(); }}>
      <input aria-label="Category (optional)" placeholder="Category (optional)" maxLength={100} value={category} onChange={event => setCategory(event.target.value)} className="w-full min-w-0 rounded border border-[var(--hair)] bg-transparent px-2 py-1.5 text-[12px]" />
      <button className={button} disabled={disabled || saving}>Save category</button><button type="button" className={button} onClick={done}>Cancel</button>
    </form>
    {error && <p role="alert" className="text-[12px] text-[var(--trip)]">{error}</p>}
  </>;
}
