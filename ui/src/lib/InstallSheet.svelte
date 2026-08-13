<script lang="ts">
  import { copy } from "./copy";
  import { isValidInstallInput, parseInstallSource, resolvedInstallSource } from "./installSource";

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

  function submit() {
    const source = resolvedInstallSource(text);
    if (!source || busy) {
      return;
    }
    onInstall(source);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      onCancel();
    }
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

<svelte:window onkeydown={onKey} />

<div class="overlay">
  <button type="button" class="backdrop" aria-label={copy.cancel} onclick={onCancel}></button>
  <div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="install-title">
    <h2 id="install-title">Install — {agentName}</h2>
    <label class="field">
      <span class="label">Source</span>
      <input type="text" bind:value={text} disabled={busy} placeholder="owner/repo or https://…" />
    </label>
    <p class="hint">{copy.installHelper}</p>
    <button type="button" class="folder" disabled={!installFolder || busy} onclick={() => void pickFolder()}>
      {copy.folder}
    </button>
    {#if !installFolder}
      <p class="hint">{copy.folderUnsupported}</p>
    {/if}
    {#if inlineError}
      <p class="err">{inlineError}</p>
    {/if}
    {#if error}
      <p class="err">{error}</p>
    {/if}
    <div class="actions">
      <button type="button" onclick={onCancel}>{copy.cancel}</button>
      <button type="button" class="primary" disabled={!valid || busy} onclick={submit}>
        {busy ? copy.installing : copy.install}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: rgb(0 0 0 / 0.45);
    cursor: pointer;
  }

  .sheet {
    position: relative;
    width: min(440px, calc(100vw - 32px));
    background: var(--plate);
    color: var(--silkscreen);
    padding: 20px;
    border: 1px solid var(--mute);
  }

  h2 {
    margin: 0 0 16px;
    font-family: "IBM Plex Sans Condensed", sans-serif;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    font-size: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .label {
    font-size: 12px;
    color: var(--mute);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  input[type="text"] {
    background: var(--well);
    color: var(--silkscreen);
    border: 1px solid var(--mute);
    padding: 8px 10px;
  }

  .hint,
  .err {
    font-size: 12px;
    margin: 8px 0 0;
  }

  .hint {
    color: var(--mute);
  }

  .err {
    color: var(--trip);
  }

  .folder {
    margin-top: 10px;
    background: var(--well);
    color: var(--silkscreen);
    border: 1px solid var(--mute);
    padding: 8px 12px;
    cursor: pointer;
  }

  .folder:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }

  .actions button {
    background: var(--well);
    color: var(--silkscreen);
    border: 1px solid var(--mute);
    padding: 8px 12px;
    cursor: pointer;
  }

  .actions button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .primary {
    background: var(--brass) !important;
    color: var(--void) !important;
    border-color: var(--brass) !important;
  }
</style>
