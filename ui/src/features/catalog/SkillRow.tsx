import { Rocker } from "@/features/agents/Rocker";
import { copy } from "$lib/copy";
import { isProjectOrigin } from "$lib/project";
import type { SkillDto } from "$lib/types";

type SkillRowProps = {
  skill: SkillDto;
  busy?: boolean;
  live?: boolean;
  onToggle: (enabled: boolean) => void;
};

export function SkillRow({ skill, busy = false, live, onToggle }: SkillRowProps) {
  const isLive = live ?? (skill.togglable ? skill.enabled : false);
  const lockedNote = isProjectOrigin(skill.origin) ? copy.skillProject : copy.skillLocked;
  const lockedLabel = isProjectOrigin(skill.origin) ? "project" : "with plugin";

  return (
    <div className="flex items-start gap-2.5 px-[11px] py-2">
      <span
        className={`mt-1.5 size-2 shrink-0 rounded-full ${
          isLive ? "bg-[var(--live)] shadow-[0_0_7px_var(--live)]" : "bg-[var(--mute)]"
        }`}
        aria-hidden="true"
      />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-semibold leading-snug break-words">{skill.name}</div>
        {skill.description ? (
          <div className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug break-words text-[var(--mute)]">
            {skill.description}
          </div>
        ) : null}
        {skill.togglable ? (
          <div className="mt-0.5 font-mono text-[10.5px] text-[var(--mute)]">user-togglable</div>
        ) : null}
      </div>
      {skill.togglable ? (
        <div className="mt-0.5 shrink-0">
          <Rocker
            size="skill"
            on={skill.enabled}
            busy={busy}
            ariaLabel={`${skill.name} ${skill.enabled ? "on" : "off"}`}
            onToggle={() => onToggle(!skill.enabled)}
          />
        </div>
      ) : (
        <span
          className="mt-1 flex min-w-[88px] shrink-0 items-center gap-[7px] font-mono text-[10.5px] text-[var(--mute)]"
          title={lockedNote}
        >
          <span className="size-2 shrink-0 rounded-full bg-[var(--mute)]" aria-hidden="true" />
          {lockedLabel}
          <span className="sr-only">{lockedNote}</span>
        </span>
      )}
    </div>
  );
}
