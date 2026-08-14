import { copy } from "$lib/copy";

type ConfirmDialogProps = {
  title: string;
  body: string;
  confirmLabel?: string;
  busy?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

export function ConfirmDialog({
  title,
  body,
  confirmLabel = copy.uninstall,
  busy = false,
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <div className="fixed inset-0 z-50" role="presentation">
      <button
        type="button"
        className="fixed inset-0 z-50 cursor-default border-0 bg-black/55 p-0"
        aria-label="Close dialog"
        onClick={onCancel}
      />
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4 pointer-events-none">
        <div
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="confirm-dialog-title"
          aria-describedby="confirm-dialog-body"
          className="pointer-events-auto w-[410px] max-w-[calc(100vw-32px)] rounded-xl border border-[var(--hair)] bg-[var(--plate)] p-[18px] shadow-[0_24px_60px_rgba(0,0,0,.5)]"
        >
          <h2 id="confirm-dialog-title" className="text-[15px] font-semibold leading-snug">
            {title}
          </h2>
          <p id="confirm-dialog-body" className="mt-2 text-[13px] text-[var(--mute)]">
            {body}
          </p>
          <footer className="mt-4 flex justify-end gap-2">
            <button
              type="button"
              className="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12.5px] text-[var(--silkscreen)]"
              onClick={onCancel}
            >
              {copy.cancel}
            </button>
            <button
              type="button"
              className="h-8 rounded-lg border border-[var(--trip)] bg-[var(--trip)] px-3.5 text-[12.5px] text-[#f7f1ea] disabled:opacity-45"
              disabled={busy}
              onClick={onConfirm}
            >
              {confirmLabel}
            </button>
          </footer>
        </div>
      </div>
    </div>
  );
}
