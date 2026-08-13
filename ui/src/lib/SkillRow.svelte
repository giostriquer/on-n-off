<script lang="ts">
  import { copy } from "./copy";
  import { isProjectOrigin } from "./project";
  import Rocker from "./Rocker.svelte";
  import type { SkillDto } from "./types";

  let {
    skill,
    busy = false,
    live = skill.togglable ? skill.enabled : false,
    onToggle,
  }: {
    skill: SkillDto;
    busy?: boolean;
    live?: boolean;
    onToggle: (enabled: boolean) => void;
  } = $props();

  const lockedNote = $derived(
    isProjectOrigin(skill.origin) ? copy.skillProject : copy.skillLocked,
  );
  const lockedLabel = $derived(isProjectOrigin(skill.origin) ? "project" : "with plugin");
</script>

<div class="flex items-center gap-2.5 px-[11px] py-2">
  <span
    class="size-2 shrink-0 rounded-full {live ? 'bg-[var(--live)] shadow-[0_0_7px_var(--live)]' : 'bg-[var(--mute)]'}"
    aria-hidden="true"
  ></span>
  <div class="min-w-0 flex-1">
    <div class="text-[13px] font-semibold leading-snug break-words">{skill.name}</div>
    {#if skill.description}
      <div class="mt-0.5 text-[11.5px] leading-snug break-words text-[var(--mute)]">{skill.description}</div>
    {/if}
    {#if !skill.togglable}
      <div class="mt-0.5 font-mono text-[10.5px] text-[var(--mute)]">
        {isProjectOrigin(skill.origin) ? "project skill" : "enabled with plugin"}
      </div>
    {:else}
      <div class="mt-0.5 font-mono text-[10.5px] text-[var(--mute)]">user-togglable</div>
    {/if}
  </div>
  {#if skill.togglable}
    <div class="shrink-0">
      <Rocker
        size="skill"
        on={skill.enabled}
        {busy}
        ariaLabel={`${skill.name} ${skill.enabled ? "on" : "off"}`}
        onToggle={() => onToggle(!skill.enabled)}
      />
    </div>
  {:else}
    <span
      class="flex min-w-[88px] shrink-0 items-center gap-[7px] font-mono text-[10.5px] text-[var(--mute)]"
      title={lockedNote}
    >
      <span class="size-2 shrink-0 rounded-full bg-[var(--mute)]" aria-hidden="true"></span>
      {lockedLabel}
      <span class="sr-only">{lockedNote}</span>
    </span>
  {/if}
</div>
