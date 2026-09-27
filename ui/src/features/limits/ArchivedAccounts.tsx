import { useId, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { removeAccountQuestion } from "@/features/accounts/AccountCardActions";
import { accountButton as button } from "@/features/accounts/AccountManager";
import { parseInvokeError } from "$lib/error";
import { useFocusHandoff } from "$lib/focusHandoff";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import type { CardAccount, LimitCard } from "./limitCards";

/** A row's actions, each of which takes the row out of the list once it goes through. */
type RowAction = "unarchive" | "remove";

/**
 * A provider's archived accounts: a collapsed "Archived (n)" list, absent while there are none.
 * Each row names the account, shows no usage, and offers Unarchive and Remove account; archived
 * accounts are never used from here.
 */
export function ArchivedAccounts({ provider, cards, blocked, onUnarchive, onRemove, focusWhenEmpty, disclosureRef }: {
  provider: AgentId;
  /** The column's archived cards (`limitColumn`), in card order. */
  cards: LimitCard[];
  /** The account controls cannot act right now (the account manager's `blocked`). */
  blocked: boolean;
  onUnarchive: (account: CardAccount) => Promise<void>;
  /** Remove account, exactly as a card's menu does it. */
  onRemove: (account: CardAccount) => Promise<void>;
  /** Where focus goes once a row's action empties the list: the column's first card's actions. */
  focusWhenEmpty?: () => HTMLElement | null | undefined;
  /** Receives the "Archived (n)" disclosure, where focus goes after a card is archived. */
  disclosureRef?: (node: HTMLButtonElement | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const disclosure = useRef<HTMLButtonElement>(null);
  const rowButtons = useRef(new Map<string, HTMLButtonElement>());
  const list = useId();
  const keys = cards.map(card => card.key);
  const rowButton = (action: RowAction, key: string | undefined) => (key === undefined ? undefined : rowButtons.current.get(`${action} ${key}`));
  // A row that leaves hands focus to the next row's same button, else the previous row's.
  const handOff = useFocusHandoff<RowAction>(keys, (index, action) =>
    [rowButton(action, keys[index]), rowButton(action, keys[index - 1]), disclosure.current, focusWhenEmpty?.()]);
  if (cards.length === 0) return null;
  const name = providerLabel(provider);
  return (
    <section aria-label={`${name} archived accounts`} className="flex flex-col gap-2">
      <button ref={node => { disclosure.current = node; disclosureRef?.(node); }} type="button" aria-expanded={open} aria-controls={list} onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1.5 self-start rounded-md px-1.5 py-1 text-[12px] font-medium text-[var(--mute)] hover:bg-[var(--wash)] hover:text-[var(--silkscreen)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--fill)]">
        <ChevronRight aria-hidden="true" className={`size-3.5 transition-transform ${open ? "rotate-90" : ""}`} />
        Archived ({cards.length})
      </button>
      <ul id={list} hidden={!open} aria-label={`Archived ${name} accounts`}
        className="m-0 list-none overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] p-0">
        {cards.flatMap(card => card.account ? [
          <ArchivedRow key={card.key} card={card} account={card.account} blocked={blocked} onUnarchive={onUnarchive} onRemove={onRemove}
            onButton={(action, node) => {
              if (node) rowButtons.current.set(`${action} ${card.key}`, node);
              else rowButtons.current.delete(`${action} ${card.key}`);
            }}
            onDone={action => handOff(card.key, action)} />,
        ] : [])}
      </ul>
    </section>
  );
}

function ArchivedRow({ card, account, blocked, onUnarchive, onRemove, onButton, onDone }: {
  card: LimitCard;
  account: CardAccount;
  blocked: boolean;
  onUnarchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
  /** The row's action buttons, where focus lands when a neighbouring row leaves. */
  onButton: (action: RowAction, node: HTMLButtonElement | null) => void;
  /** `action` went through, so the row is leaving the list. */
  onDone: (action: RowAction) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const remove = useRef<HTMLButtonElement>(null);
  const label = card.identity.label;
  const disabled = blocked || working;
  async function run(action: RowAction) {
    setError(null); setWorking(true);
    try {
      await (action === "unarchive" ? onUnarchive : onRemove)(account);
      setConfirming(false);
      onDone(action);
    } catch (reason) {
      setError(parseInvokeError(reason).message);
    } finally {
      setWorking(false);
    }
  }
  function cancel() {
    setConfirming(false); setError(null);
    remove.current?.focus();
  }
  return (
    <li className="border-t border-[var(--hair)] px-3.5 py-2.5 first:border-t-0">
      <div className="flex flex-wrap items-center gap-2">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-semibold" title={label}>{label}</div>
          {card.identity.category && <div className="break-words text-[11px] text-[var(--mute)]">{card.identity.category}</div>}
        </div>
        <button ref={node => onButton("unarchive", node)} type="button" className={button} disabled={disabled} aria-label={`Unarchive ${label}`} onClick={() => void run("unarchive")}>Unarchive</button>
        <button ref={node => { remove.current = node; onButton("remove", node); }} type="button" className={button} disabled={disabled} aria-label={`Remove account ${label}`} onClick={() => { setError(null); setConfirming(true); }}>Remove account</button>
      </div>
      {confirming && <div role="group" aria-label="Confirm account action" className="mt-2 flex flex-col gap-2 text-[12px]"
        onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); cancel(); } }}>
        <p className="m-0">{removeAccountQuestion(label)}</p>
        <div className="flex gap-2">
          <button type="button" autoFocus className={button} disabled={disabled} onClick={() => void run("remove")}>Confirm removal</button>
          <button type="button" className={button} onClick={cancel}>Cancel</button>
        </div>
      </div>}
      {error && <p role="alert" className="mt-1.5 mb-0 text-[12px] text-[var(--trip)]">{error}</p>}
    </li>
  );
}
