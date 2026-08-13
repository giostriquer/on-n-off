<script lang="ts">
  type Size = "tab" | "plugin" | "skill" | "master" | "theme";

  let {
    on = false,
    size = "plugin",
    busy = false,
    disabled = false,
    dangerOff = false,
    onLabel = "ON",
    offLabel = "OFF",
    ariaLabel,
    onToggle,
  }: {
    on: boolean;
    size?: Size;
    busy?: boolean;
    disabled?: boolean;
    dangerOff?: boolean;
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
  class:dangerOff
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
    border: 1px solid var(--hair);
    border-radius: 11px;
    background: var(--plate);
    color: var(--mute);
    cursor: pointer;
    user-select: none;
    letter-spacing: 0.03em;
    font-family: "Instrument Sans", sans-serif;
    font-weight: 600;
  }

  .rocker.tab,
  .rocker.theme {
    border-radius: 8px;
  }

  .rocker.tab {
    height: 30px;
    min-width: 92px;
    font-size: 11.5px;
  }

  .rocker.plugin {
    height: 28px;
    min-width: 88px;
    font-size: 11px;
  }

  .rocker.skill {
    height: 22px;
    min-width: 70px;
    font-size: 9.5px;
  }

  .rocker.theme {
    height: 22px;
    min-width: 72px;
    font-size: 9.5px;
  }

  .rocker.master {
    height: 30px;
    width: 100%;
    font-size: 10.5px;
  }

  .half {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 8px;
    overflow: hidden;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .rocker.on .on-side {
    background: var(--brass);
    color: var(--void);
  }

  .rocker:not(.on) .off {
    background: var(--well);
    color: var(--silkscreen);
  }

  .rocker.dangerOff:not(.on) .off {
    background: var(--trip);
    color: #f7f1ea;
  }

  .rocker:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .rocker.busy {
    opacity: 0.6;
  }
</style>
