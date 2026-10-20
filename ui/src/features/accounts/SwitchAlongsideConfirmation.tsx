import { actionButton as button } from "@/components/actionButton";

export function SwitchAlongsideConfirmation({ product, clients, disabled, onSwitch, onCancel }: {
  product: string; clients: string[]; disabled: boolean; onSwitch: () => void; onCancel: () => void;
}) {
  return <div role="group" aria-label={`Confirm switching while ${product} is running`} className="flex w-full flex-col gap-2 text-[12px]"
    onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); onCancel(); } }}>
    <p className="m-0">{product} is still running in {clients.join(", ")}. Those sessions keep using the current account until you restart them. Don't sign out or sign in again from them: that can revoke saved logins.</p>
    <div className="flex gap-2"><button className={button} disabled={disabled} onClick={onSwitch}>Switch anyway</button><button autoFocus className={button} onClick={onCancel}>Cancel</button></div>
  </div>;
}
