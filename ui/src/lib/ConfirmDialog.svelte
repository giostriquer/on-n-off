<script lang="ts">
  import { copy } from "./copy";

  let {
    title,
    body,
    confirmLabel = copy.uninstall,
    busy = false,
    onCancel,
    onConfirm,
  }: {
    title: string;
    body: string;
    confirmLabel?: string;
    busy?: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      onCancel();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="overlay">
  <button type="button" class="backdrop" aria-label={copy.cancel} onclick={onCancel}></button>
  <div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="confirm-title">
    <h2 id="confirm-title">{title}</h2>
    <p>{body}</p>
    <div class="actions">
      <button type="button" onclick={onCancel}>{copy.cancel}</button>
      <button type="button" class="trip" disabled={busy} onclick={onConfirm}>{confirmLabel}</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 21;
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
    width: min(400px, calc(100vw - 32px));
    background: var(--plate);
    padding: 20px;
    border: 1px solid var(--mute);
  }

  h2 {
    margin: 0 0 10px;
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-size: 16px;
  }

  p {
    margin: 0;
    color: var(--mute);
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

  .trip {
    background: var(--trip);
    border-color: var(--trip);
    color: #f7f1ea;
  }

  .actions button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
