<script lang="ts">
  type Size = "tab" | "plugin" | "skill";

  let {
    on = false,
    size = "plugin",
    busy = false,
    disabled = false,
    onLabel = "ON",
    offLabel = "OFF",
    ariaLabel,
    onToggle,
  }: {
    on: boolean;
    size?: Size;
    busy?: boolean;
    disabled?: boolean;
    onLabel?: string;
    offLabel?: string;
    ariaLabel: string;
    onToggle: () => void;
  } = $props();

  const locked = $derived(disabled || busy);

  function activate() {
    if (locked) {
      return;
    }
    onToggle();
  }
</script>

<button
  type="button"
  class="rocker {size}"
  class:on
  class:busy
  disabled={locked}
  aria-pressed={on}
  aria-label={ariaLabel}
  aria-busy={busy}
  onclick={activate}
>
  <span class="half off">{offLabel}</span>
  <span class="half on-side">{onLabel}</span>
</button>

<style>
  .rocker {
    display: inline-grid;
    grid-template-columns: 1fr 1fr;
    padding: 0;
    border: 1px solid var(--mute);
    background: var(--plate);
    color: var(--mute);
    cursor: pointer;
    user-select: none;
    letter-spacing: 0.06em;
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-weight: 600;
  }

  .rocker.tab {
    height: 36px;
    min-width: 132px;
    font-size: 13px;
  }

  .rocker.plugin {
    height: 28px;
    min-width: 88px;
    font-size: 11px;
  }

  .rocker.skill {
    height: 22px;
    min-width: 72px;
    font-size: 10px;
  }

  .half {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 8px;
  }

  .rocker.on .on-side {
    background: var(--brass);
    color: var(--void);
  }

  .rocker:not(.on) .off {
    background: var(--well);
    color: var(--silkscreen);
  }

  .rocker:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .rocker.busy {
    opacity: 0.6;
  }
</style>
