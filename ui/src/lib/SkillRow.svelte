<script lang="ts">
  import { copy } from "./copy";
  import Rocker from "./Rocker.svelte";
  import type { SkillDto } from "./types";

  let {
    skill,
    source = null,
    busy = false,
    onToggle,
  }: {
    skill: SkillDto;
    source?: string | null;
    busy?: boolean;
    onToggle: (enabled: boolean) => void;
  } = $props();
</script>

<div class="row" class:locked={!skill.togglable}>
  <div class="meta">
    <div class="name">{skill.name}</div>
    {#if skill.description && !source}
      <div class="desc">{skill.description}</div>
    {/if}
    {#if !skill.togglable}
      <div class="reason">{copy.skillLocked}</div>
    {/if}
  </div>
  {#if source}
    <span class="source">{source}</span>
  {/if}
  {#if skill.togglable}
    <Rocker
      size="skill"
      on={skill.enabled}
      {busy}
      ariaLabel={`${skill.name} ${skill.enabled ? "on" : "off"}`}
      onToggle={() => onToggle(!skill.enabled)}
    />
  {:else}
    <span class="pip" aria-hidden="true"></span>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
  }

  .meta {
    flex: 1;
    min-width: 0;
  }

  .name {
    font-family: "IBM Plex Sans Condensed", sans-serif;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .desc,
  .reason,
  .source {
    color: var(--mute);
    font-size: 12px;
  }

  .reason {
    margin-top: 2px;
  }

  .source {
    align-self: center;
  }

  .pip {
    width: 8px;
    height: 8px;
    margin-top: 6px;
    border-radius: 50%;
    background: var(--mute);
    flex-shrink: 0;
  }
</style>
