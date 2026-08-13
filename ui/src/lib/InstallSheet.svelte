<script lang="ts">
  import { Dialog } from "@skeletonlabs/skeleton-svelte";
  import { copy } from "./copy";
  import { installHint, isValidInstallInput, parseInstallSource, resolvedInstallSource } from "./installSource";

  let {
    agentName,
    busy = false,
    error = null,
    installFolder = false,
    onCancel,
    onInstall,
    onPickFolder = async () => null,
  }: {
    agentName: string;
    busy?: boolean;
    error?: string | null;
    installFolder?: boolean;
    onCancel: () => void;
    onInstall: (source: string) => void;
    onPickFolder?: () => Promise<string | null>;
  } = $props();

  let text = $state("");

  const parsed = $derived(parseInstallSource(text));
  const valid = $derived(isValidInstallInput(text));
  const inlineError = $derived(text.trim() && "error" in parsed ? parsed.error : null);
  const hint = $derived(installHint(parsed));

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
      text = dir;
    }
  }
</script>

<Dialog
  open
  onOpenChange={(details) => {
    if (!details.open) {
      onCancel();
    }
  }}
>
  <Dialog.Backdrop class="fixed inset-0 z-50 bg-black/55" />
  <Dialog.Positioner class="fixed inset-0 z-50 flex items-center justify-center p-4">
    <Dialog.Content class="w-[470px] max-w-[calc(100vw-32px)] overflow-hidden rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] shadow-[var(--drop)]">
      <header class="flex items-center gap-2.5 border-b border-[var(--hair)] px-4 py-[13px]">
        <span class="size-2 shrink-0 bg-[var(--fill)]" aria-hidden="true"></span>
        <Dialog.Title class="text-[14px] font-semibold tracking-[0.05em] uppercase">
          Install — {agentName}
        </Dialog.Title>
      </header>
      <div class="flex flex-col gap-2.5 p-4">
        <span class="text-[10px] font-semibold tracking-[0.05em] text-[var(--mute)]">SOURCE</span>
        <input
          class="w-full rounded-lg border border-[var(--hair)] bg-[var(--well)] px-2.5 py-[9px] font-mono text-[13px] text-[var(--silkscreen)]"
          type="text"
          bind:value={text}
          disabled={busy}
          aria-label="Install source"
          placeholder="name@marketplace, owner/repo, or npx skills add …"
        />
        <p class="text-[11.5px] text-[var(--mute)]">{hint}</p>
        <div class="flex items-center gap-2.5">
          <button
            type="button"
            class="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3 text-[12.5px] text-[var(--silkscreen)] disabled:opacity-45"
            disabled={!installFolder || busy}
            onclick={() => void pickFolder()}
          >
            {copy.folder}
          </button>
          {#if !installFolder}
            <span class="flex-1 text-[11.5px] text-[var(--mute)]">{copy.folderUnsupported}</span>
          {/if}
        </div>
        {#if inlineError}
          <p class="text-[13px] text-[var(--trip)]" role="alert">{inlineError}</p>
        {/if}
        {#if error}
          <p class="border-l-[3px] border-[var(--trip)] bg-[var(--well)] px-2.5 py-2 font-mono text-xs text-[var(--silkscreen)]" role="alert">
            {error}
          </p>
        {/if}
        <footer class="mt-1 flex justify-end gap-2">
          <Dialog.CloseTrigger
            class="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12.5px] text-[var(--silkscreen)]"
          >
            {copy.cancel}
          </Dialog.CloseTrigger>
          <button
            type="button"
            class="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-4 text-[11.5px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] disabled:opacity-45"
            disabled={!valid || busy}
            onclick={submit}
          >
            {busy ? copy.installing : copy.install}
          </button>
        </footer>
      </div>
    </Dialog.Content>
  </Dialog.Positioner>
</Dialog>
