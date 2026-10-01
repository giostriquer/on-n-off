import { useId, useRef, useState, type RefCallback } from "react";
import { ChevronRight } from "lucide-react";
import { removeAccountQuestion } from "@/features/accounts/AccountCardActions";
import { accountButton as button } from "@/features/accounts/AccountManager";
import { parseInvokeError } from "$lib/error";
import { useFocusHandoff } from "$lib/focusHandoff";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import type { CardAccount, LimitCard } from "./limitCards";

type RowAction = "unarchive" | "remove";

export function ArchivedAccounts({ provider, cards, blocked, onUnarchive, onRemove, focusWhenEmpty, disclosureRef }: {
  provider: AgentId;
  cards: LimitCard[];
  blocked: boolean;
  onUnarchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
  focusWhenEmpty?: () => HTMLElement | null | undefined;
  disclosureRef?: RefCallback<HTMLButtonElement>;
}) {
  const [open, setOpen] = useState(false);
  const disclosure = useRef<HTMLButtonElement>(null);
  const rowButtons = useRef(new Map<string, HTMLButtonElement>());
  const list = useId();
  const keys = cards.map(card => card.key);
  const rowButton = (action: RowAction, key: string | undefined) => (key === undefined ? undefined : rowButtons.current.get(`${action} ${key}`));
  const rowButtonRef = (action: RowAction, key: string): RefCallback<HTMLButtonElement> => node => {
    if (node) rowButtons.current.set(`${action} ${key}`, node);
    else rowButtons.current.delete(`${action} ${key}`);
  };
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
            unarchiveRef={rowButtonRef("unarchive", card.key)} removeRef={rowButtonRef("remove", card.key)}
            onDone={action => handOff(card.key, action)} />,
        ] : [])}
      </ul>
    </section>
  );
}

function ArchivedRow({ card, account, blocked, onUnarchive, onRemove, unarchiveRef, removeRef, onDone }: {
  card: LimitCard;
  account: CardAccount;
  blocked: boolean;
  onUnarchive: (account: CardAccount) => Promise<void>;
  onRemove: (account: CardAccount) => Promise<void>;
  unarchiveRef: RefCallback<HTMLButtonElement>;
  removeRef: RefCallback<HTMLButtonElement>;
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
        <button ref={unarchiveRef} type="button" className={button} disabled={disabled} aria-label={`Unarchive ${label}`} onClick={() => void run("unarchive")}>Unarchive</button>
        <button ref={node => { remove.current = node; removeRef(node); }} type="button" className={button} disabled={disabled} aria-label={`Remove account ${label}`} onClick={() => { setError(null); setConfirming(true); }}>Remove account</button>
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
