import { useState } from "react";
import { copy } from "$lib/copy";
import { installHint, isValidInstallInput, parseInstallSource, resolvedInstallSource } from "$lib/installSource";

type InstallSheetProps = {
  agentName: string;
  busy?: boolean;
  error?: string | null;
  installFolder?: boolean;
  onCancel: () => void;
  onInstall: (source: string) => void;
  onPickFolder?: () => Promise<string | null>;
};

export function InstallSheet({
  agentName,
  busy = false,
  error = null,
  installFolder = false,
  onCancel,
  onInstall,
  onPickFolder = async () => null,
}: InstallSheetProps) {
  const [text, setText] = useState("");
  const parsed = parseInstallSource(text);
  const valid = isValidInstallInput(text);
  const inlineError = text.trim() && "error" in parsed ? parsed.error : null;
  const hint = installHint(parsed);

  function submit() {
    const source = resolvedInstallSource(text);
    if (!source || busy) {
      return;
    }
    onInstall(source);
  }

  async function pickFolder() {
    if (!installFolder || busy) {
      return;
    }
    const dir = await onPickFolder();
    if (dir) {
      setText(dir);
    }
  }

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
          role="dialog"
          aria-modal="true"
          aria-labelledby="install-sheet-title"
          className="pointer-events-auto w-[470px] max-w-[calc(100vw-32px)] overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] shadow-[var(--drop)]"
        >
          <header className="flex items-center gap-2.5 border-b border-[var(--hair)] px-4 py-[13px]">
            <span className="size-2 shrink-0 bg-[var(--fill)]" aria-hidden="true" />
            <h2 id="install-sheet-title" className="text-[14px] font-semibold tracking-[0.05em] uppercase">
              Install — {agentName}
            </h2>
          </header>
          <div className="flex flex-col gap-2.5 p-4">
            <span className="text-[10px] font-semibold tracking-[0.05em] text-[var(--mute)]">SOURCE</span>
            <input
              className="w-full rounded-lg border border-[var(--hair)] bg-[var(--well)] px-2.5 py-[9px] font-mono text-[13px] text-[var(--silkscreen)]"
              type="text"
              value={text}
              disabled={busy}
              aria-label="Install source"
              placeholder="name@marketplace, owner/repo, or npx skills add …"
              onChange={(event) => setText(event.target.value)}
            />
            <p className="text-[11.5px] text-[var(--mute)]">{hint}</p>
            <div className="flex items-center gap-2.5">
              <button
                type="button"
                className="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3 text-[12.5px] text-[var(--silkscreen)] disabled:opacity-45"
                disabled={!installFolder || busy}
                onClick={() => void pickFolder()}
              >
                {copy.folder}
              </button>
              {!installFolder ? (
                <span className="flex-1 text-[11.5px] text-[var(--mute)]">{copy.folderUnsupported}</span>
              ) : null}
            </div>
            {inlineError ? (
              <p className="text-[13px] text-[var(--trip)]" role="alert">
                {inlineError}
              </p>
            ) : null}
            {error ? (
              <p
                className="border-l-[3px] border-[var(--trip)] bg-[var(--well)] px-2.5 py-2 font-mono text-xs text-[var(--silkscreen)]"
                role="alert"
              >
                {error}
              </p>
            ) : null}
            <footer className="mt-1 flex justify-end gap-2">
              <button
                type="button"
                className="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12.5px] text-[var(--silkscreen)]"
                onClick={onCancel}
              >
                {copy.cancel}
              </button>
              <button
                type="button"
                className="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-4 text-[11.5px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] disabled:opacity-45"
                disabled={!valid || busy}
                onClick={submit}
              >
                {busy ? copy.installing : copy.install}
              </button>
            </footer>
          </div>
        </div>
      </div>
    </div>
  );
}
