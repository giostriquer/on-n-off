import { useState } from "react";
import { copy } from "$lib/copy";
import {
  githubRepoFromSource,
  installHint,
  isValidInstallInput,
  parseInstallSource,
  resolvedInstallSource,
} from "$lib/installSource";
import type { AgentId, AgentInfo, InstallItemsRequest, InstallItemsResult, ProjectDto } from "$lib/types";
import { MarketplaceInstall } from "./MarketplaceInstall";
import { SheetError, SheetFooter } from "./SheetChrome";

type InstallSheetProps = {
  agentName: string;
  busy: boolean;
  error: string | null;
  installFolder: boolean;
  visibleAgents: AgentInfo[];
  currentAgentId: AgentId;
  projects: ProjectDto[];
  currentScopePath: string | null;
  onCancel: () => void;
  onInstall: (source: string) => void;
  onInstallItems: (request: InstallItemsRequest) => Promise<InstallItemsResult | null>;
  onPickFolder: () => Promise<string | null>;
};

export function InstallSheet({
  agentName,
  busy,
  error,
  installFolder,
  visibleAgents,
  currentAgentId,
  projects,
  currentScopePath,
  onCancel,
  onInstall,
  onInstallItems,
  onPickFolder,
}: InstallSheetProps) {
  const [text, setText] = useState("");
  const parsed = parseInstallSource(text);
  const valid = isValidInstallInput(text);
  const inlineError = text.trim() && "error" in parsed ? parsed.error : null;
  const hint = installHint(parsed);
  const repo = githubRepoFromSource(parsed);

  function submitPlugin() {
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
          className={`pointer-events-auto max-h-[calc(100vh-32px)] max-w-[calc(100vw-32px)] overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] shadow-[var(--drop)] ${
            repo ? "w-[560px]" : "w-[470px]"
          }`}
        >
          <header className="flex items-center gap-2.5 border-b border-[var(--hair)] px-4 py-[13px]">
            <span className="size-2 shrink-0 bg-[var(--fill)]" aria-hidden="true" />
            <h2 id="install-sheet-title" className="text-[14px] font-semibold tracking-[0.05em] uppercase">
              Install — {agentName}
            </h2>
          </header>
          <div className="sheet-scroll flex max-h-[calc(100vh-90px)] flex-col gap-2.5 p-4">
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
            {repo ? (
              <MarketplaceInstall
                key={`${repo.owner}/${repo.repo}@${repo.ref ?? ""}`}
                repo={repo}
                agentName={agentName}
                hint={hint}
                busy={busy}
                error={error}
                visibleAgents={visibleAgents}
                currentAgentId={currentAgentId}
                projects={projects}
                currentScopePath={currentScopePath}
                onCancel={onCancel}
                onInstallPlugin={submitPlugin}
                onInstallItems={onInstallItems}
                onPickFolder={onPickFolder}
              />
            ) : (
              <>
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
                <SheetError message={error} />
                <SheetFooter
                  submitLabel={busy ? copy.installing : copy.install}
                  submitDisabled={!valid || busy}
                  onCancel={onCancel}
                  onSubmit={submitPlugin}
                />
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
