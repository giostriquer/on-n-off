import { copy } from "$lib/copy";

export function SheetFooter({
  cancelLabel = copy.cancel,
  submitLabel,
  submitDisabled,
  summary = null,
  onCancel,
  onSubmit,
}: {
  cancelLabel?: string;
  submitLabel: string;
  submitDisabled: boolean;
  summary?: string | null;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  return (
    <footer className="mt-1 flex items-center justify-end gap-2">
      {summary ? <span className="mr-auto text-[11px] text-[var(--mute)]">{summary}</span> : null}
      <button
        type="button"
        className="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12px] text-[var(--silkscreen)]"
        onClick={onCancel}
      >
        {cancelLabel}
      </button>
      <button
        type="button"
        className="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-4 text-[12px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] disabled:opacity-45"
        disabled={submitDisabled}
        onClick={onSubmit}
      >
        {submitLabel}
      </button>
    </footer>
  );
}

export function SheetError({ message }: { message: string | null }) {
  if (!message) {
    return null;
  }
  return (
    <p
      className="border-l-[3px] border-[var(--trip)] bg-[var(--well)] px-2.5 py-2 font-mono text-xs text-[var(--silkscreen)]"
      role="alert"
    >
      {message}
    </p>
  );
}
