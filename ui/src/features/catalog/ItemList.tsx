import { memo, useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { Rocker } from "@/features/agents/Rocker";
import { ManagedItemStrip } from "./ManagedItemStrip";
import { SkillRow } from "./SkillRow";
import { copy } from "$lib/copy";
import {
  formatPluginVersion,
  canUninstallPlugin,
  pluginOutOfSync,
  pluginVersionNote,
  skillIsLive,
} from "$lib/catalog";
import { isProjectOrigin } from "$lib/project";
import type { AgentTabDto, ItemStatus, PluginDto, SkillDto } from "$lib/types";

type Chip = "all" | "on" | "off" | "behind";

const noop = () => {};

type ItemListBaseProps = {
  tab: AgentTabDto;
  filterQuery?: string;
  expandedIds: Set<string>;
  cliOk: boolean;
  pluginToggle: boolean;
  busy?: boolean;
  onToggleExpand: (pluginId: string) => void;
  onTogglePlugin: (plugin: PluginDto, enabled: boolean) => void;
  onToggleSkill: (skill: SkillDto, enabled: boolean) => void;
  onUninstall: (plugin: PluginDto) => void;
  onUpdate?: (plugin: PluginDto) => void;
  /** Managed-item record for a skill on-n-off installed from a marketplace, if any. */
  statusFor?: (skill: SkillDto) => ItemStatus | undefined;
  onUpdateItem?: (status: ItemStatus) => void;
  onRemoveItem?: (status: ItemStatus) => void;
  headerActions?: ReactNode;
};

type ItemListProps = ItemListBaseProps &
  ({ kind: "plugin"; items: PluginDto[] } | { kind: "skill"; items: SkillDto[] });

function applyChip<T>(items: T[], next: Chip, enabled: (item: T) => boolean): T[] {
  if (next === "on") {
    return items.filter((item) => enabled(item));
  }
  if (next === "off") {
    return items.filter((item) => !enabled(item));
  }
  return items;
}

type SkillCardProps = {
  skill: SkillDto;
  live: boolean;
  busy: boolean;
  status?: ItemStatus;
  onToggleSkill: (skill: SkillDto, enabled: boolean) => void;
  onUpdateItem?: (status: ItemStatus) => void;
  onRemoveItem?: (status: ItemStatus) => void;
};

const SkillCard = memo(function SkillCard({
  skill,
  live,
  busy,
  status,
  onToggleSkill,
  onUpdateItem,
  onRemoveItem,
}: SkillCardProps) {
  const lockedNote = isProjectOrigin(skill.origin) ? copy.skillProject : copy.skillLocked;
  return (
    <article className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
      <div className="flex items-start gap-3 px-3 py-[11px]">
        <span
          className={`mt-2 size-2 shrink-0 rounded-full ${
            live ? "bg-[var(--live)] shadow-[0_0_7px_var(--live)]" : "bg-[var(--mute)]"
          }`}
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
            <span className="text-[16px]/[1.15] font-semibold break-words">{skill.name}</span>
            <span className="shrink-0 border border-[var(--mute)] px-1.5 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] text-[var(--mute)]">
              {isProjectOrigin(skill.origin) ? "Project skill" : skill.pluginId ? "Plugin skill" : "User skill"}
            </span>
          </div>
          <div className="mt-0.5 line-clamp-3 text-[11.5px] leading-snug break-words text-[var(--mute)]">
            {skill.description || skill.id}
          </div>
        </div>
        {skill.togglable ? (
          <div className="mt-0.5 shrink-0">
            <Rocker
              size="plugin"
              on={skill.enabled}
              busy={busy}
              ariaLabel={`${skill.name} ${skill.enabled ? "on" : "off"}`}
              onToggle={() => onToggleSkill(skill, !skill.enabled)}
            />
          </div>
        ) : (
          <span
            className="mt-1 flex min-w-[88px] shrink-0 items-center gap-[7px] font-mono text-[10.5px] text-[var(--mute)]"
            title={lockedNote}
          >
            <span className="size-2 shrink-0 rounded-full bg-[var(--mute)]" aria-hidden="true" />
            {isProjectOrigin(skill.origin) ? "project" : "with plugin"}
            <span className="sr-only">{lockedNote}</span>
          </span>
        )}
      </div>
      {status ? (
        <ManagedItemStrip
          status={status}
          busy={busy}
          onUpdateItem={onUpdateItem ?? noop}
          onRemoveItem={onRemoveItem ?? noop}
        />
      ) : null}
    </article>
  );
});

export function ItemList({
  kind,
  tab,
  items,
  filterQuery = "",
  expandedIds,
  cliOk,
  pluginToggle,
  busy = false,
  onToggleExpand,
  onTogglePlugin,
  onToggleSkill,
  onUninstall,
  onUpdate,
  statusFor,
  onUpdateItem,
  onRemoveItem,
  headerActions,
}: ItemListProps) {
  const [chip, setChip] = useState<Chip>("all");

  const pluginPool = kind === "plugin" ? items : [];
  const plugins =
    chip === "behind"
      ? pluginPool.filter((plugin) => pluginOutOfSync(plugin))
      : applyChip(pluginPool, chip, (plugin) => plugin.enabled);
  const skillPool = kind === "skill" ? items : [];
  const skills = applyChip(skillPool, chip, (skill) => skillIsLive(skill, tab));

  const title = kind === "plugin" ? "Installed plugins" : "Skills library";
  const subtitle =
    kind === "plugin"
      ? `${tab.plugins.filter((plugin) => plugin.enabled).length} live · installed from git, marketplace or folder`
      : "plugin skills follow their plugin · user skills toggle · project skills stay with the repo";
  const chips: Chip[] = kind === "plugin" ? ["all", "on", "off", "behind"] : ["all", "on", "off"];

  return (
    <div className="flex flex-col gap-3.5 px-5 pt-[18px] pb-[26px]">
      <header className="flex items-baseline gap-3">
        <h2 className="m-0 text-[17px] font-semibold tracking-[0.05em] uppercase">{title}</h2>
        <span className="font-mono text-[12px]/[1.3] text-[var(--mute)]">{subtitle}</span>
        <div className="flex-1" />
        {headerActions}
        <div className="flex border border-[var(--hair)]" role="group" aria-label="Filter list">
          {chips.map((next) => (
            <button
              key={next}
              type="button"
              className={`h-[26px] rounded-none border-0 px-3 text-[10.5px] font-semibold tracking-[0.03em] uppercase ${
                chip === next ? "bg-[var(--well)] text-[var(--silkscreen)]" : "bg-transparent text-[var(--mute)]"
              }`}
              aria-pressed={chip === next}
              onClick={() => setChip(next)}
            >
              {next}
            </button>
          ))}
        </div>
      </header>

      {kind === "plugin" ? (
        pluginPool.length === 0 ? (
          <p className="text-[13px] text-[var(--mute)]">
            {filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyPlugins}
          </p>
        ) : plugins.length === 0 ? (
          <p className="text-[13px] text-[var(--mute)]">
            {chip === "behind" ? "No plugins behind." : chip === "on" ? "No plugins on." : "No plugins off."}
          </p>
        ) : (
          <div className="flex flex-col gap-1.5">
            {plugins.map((plugin) => {
              const open = expandedIds.has(plugin.id);
              return (
                <article key={plugin.id} className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
                  <div className="flex items-start gap-3 px-3 py-[11px]">
                    <button
                      type="button"
                      className="mt-0.5 flex min-h-6 min-w-6 shrink-0 items-center justify-center border-0 bg-transparent p-0 text-[var(--silkscreen)]"
                      aria-expanded={open}
                      aria-label={open ? `Collapse ${plugin.name}` : `Expand ${plugin.name}`}
                      onClick={() => onToggleExpand(plugin.id)}
                    >
                      <ChevronRight
                        className={`size-4 transition-transform duration-150 ${open ? "rotate-90" : ""}`}
                        aria-hidden="true"
                      />
                    </button>
                    <span
                      className={`mt-2 size-2 shrink-0 rounded-full ${
                        plugin.enabled ? "bg-[var(--live)] shadow-[0_0_7px_var(--live)]" : "bg-[var(--mute)]"
                      }`}
                      aria-hidden="true"
                    />
                    <div className="min-w-0 flex-1 basis-[238px]">
                      <div className="text-[16px]/[1.15] font-semibold break-words">{plugin.name}</div>
                      <div
                        className="mt-0.5 truncate font-mono text-[11px]/[1.4] text-[var(--mute)]"
                        title={plugin.source}
                      >
                        {plugin.source}
                      </div>
                    </div>
                    <div className="mt-0.5 w-[112px] shrink-0">
                      {plugin.version ? (
                        <>
                          <div className="font-mono text-[12px]/[1.3] font-medium text-[var(--silkscreen)]">
                            {formatPluginVersion(plugin.version)}
                          </div>
                          <div
                            className={`font-mono text-[10.5px]/[1.3] ${
                              pluginOutOfSync(plugin) ? "text-[var(--warn)]" : "text-[var(--mute)]"
                            }`}
                          >
                            {pluginVersionNote(plugin)}
                          </div>
                        </>
                      ) : (
                        <div className="font-mono text-[12px]/[1.3] text-[var(--mute)]">—</div>
                      )}
                    </div>
                    {pluginOutOfSync(plugin) ? (
                      <button
                        type="button"
                        className="mt-0.5 h-[26px] shrink-0 border border-[var(--warn)] bg-transparent px-3 text-[10.5px] font-semibold tracking-[0.03em] text-[var(--warn)] uppercase disabled:opacity-45"
                        disabled={!cliOk || busy}
                        aria-label={`Update ${plugin.name}`}
                        onClick={() => onUpdate?.(plugin)}
                      >
                        {copy.update}
                      </button>
                    ) : null}
                    <div className="mt-0.5 ml-auto shrink-0">
                      <Rocker
                        size="plugin"
                        on={plugin.enabled}
                        busy={busy}
                        disabled={!pluginToggle || !plugin.togglable}
                        ariaLabel={`${plugin.name} ${plugin.enabled ? "on" : "off"}`}
                        onToggle={() => onTogglePlugin(plugin, !plugin.enabled)}
                      />
                    </div>
                  </div>
                  {open ? (
                    <div className="mx-3 mb-3 ml-[46px] border border-[var(--hair)] bg-[var(--well)]">
                      <div className="flex items-center gap-2.5 border-b border-[var(--hair)] px-[11px] py-2">
                        <span
                          className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-[var(--mute)]"
                          title={plugin.id}
                        >
                          {plugin.id}
                        </span>
                        <button
                          type="button"
                          className="h-6 rounded-[11px] border border-[var(--hair)] bg-[var(--plate)] px-2.5 text-[10px] font-semibold tracking-[0.03em] text-[var(--trip)] uppercase disabled:opacity-45"
                          aria-label={`Uninstall ${plugin.name}`}
                          disabled={!cliOk || busy || !canUninstallPlugin(plugin)}
                          onClick={() => onUninstall(plugin)}
                        >
                          Uninstall
                        </button>
                      </div>
                      {plugin.skills.map((skill) => (
                        <SkillRow
                          key={skill.id}
                          skill={skill}
                          live={skill.togglable ? skill.enabled : plugin.enabled}
                          busy={busy}
                          onToggle={(enabled) => onToggleSkill(skill, enabled)}
                        />
                      ))}
                    </div>
                  ) : null}
                </article>
              );
            })}
          </div>
        )
      ) : skillPool.length === 0 ? (
        <p className="text-[13px] text-[var(--mute)]">
          {filterQuery.trim() ? copy.filterMiss(filterQuery) : copy.emptyUserSkills}
        </p>
      ) : skills.length === 0 ? (
        <p className="text-[13px] text-[var(--mute)]">{chip === "on" ? "No skills on." : "No skills off."}</p>
      ) : (
        <div className="flex flex-col gap-1.5">
          {skills.map((skill) => (
            <SkillCard
              key={skill.id}
              skill={skill}
              live={
                skill.togglable
                  ? skill.enabled
                  : (tab.plugins.find((plugin) => plugin.id === skill.pluginId)?.enabled ?? false)
              }
              busy={busy}
              status={statusFor?.(skill)}
              onToggleSkill={onToggleSkill}
              onUpdateItem={onUpdateItem}
              onRemoveItem={onRemoveItem}
            />
          ))}
        </div>
      )}
    </div>
  );
}
