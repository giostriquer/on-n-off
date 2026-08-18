import { copy } from "$lib/copy";
import { summarizeOutcomes } from "$lib/itemOutcomes";
import { agentsAllowed, entryKey, groupKeys, toggleGroup, type MarketplaceAction } from "$lib/marketplaceSelection";
import type {
  AgentId,
  AgentInfo,
  InstallItemsResult,
  ItemKind,
  ItemOutcome,
  ItemScope,
  MarketplaceEntry,
  MarketplaceInspect,
  MarketplacePlugin,
  ProjectDto,
} from "$lib/types";
import { ProviderChips, ScopePicker } from "./InstallTargets";

/** Everything the user chooses in the marketplace step, owned by `MarketplaceInstall`. */
export type MarketplaceSelection = {
  action: MarketplaceAction;
  keys: ReadonlySet<string>;
  filter: string;
  providers: AgentId[];
  scope: ItemScope;
};

export type MarketplaceBrowserProps = {
  inspect: MarketplaceInspect;
  selection: MarketplaceSelection;
  onChange: (patch: Partial<MarketplaceSelection>) => void;
  visibleAgents: AgentInfo[];
  projects: ProjectDto[];
  onPickFolder: () => Promise<string | null>;
  result: InstallItemsResult | null;
  busy: boolean;
  onOverwriteConflicts: () => void;
};

const ACTIONS: { id: MarketplaceAction; label: string; hint: string }[] = [
  { id: "plugin", label: copy.actionPlugin, hint: copy.actionPluginHint },
  { id: "all", label: copy.actionAll, hint: copy.actionAllHint },
  { id: "selected", label: copy.actionSelected, hint: copy.actionSelectedHint },
];

export function MarketplaceBrowser({
  inspect,
  selection,
  onChange,
  visibleAgents,
  projects,
  onPickFolder,
  result,
  busy,
  onOverwriteConflicts,
}: MarketplaceBrowserProps) {
  const { action, keys, filter, providers, scope } = selection;
  const local = action !== "plugin";
  const canAgents = agentsAllowed(providers);
  const wantsAgents = inspect.plugins.some((plugin) => plugin.supported && plugin.agents.length > 0);

  return (
    <div className="flex flex-col gap-3">
      <fieldset className="flex flex-col gap-1.5 border-0 p-0" aria-label="Marketplace action">
        {ACTIONS.map((entry) => (
          <label
            key={entry.id}
            className={`flex cursor-pointer items-start gap-2.5 rounded-lg border px-2.5 py-2 ${
              action === entry.id ? "border-[var(--fill)] bg-[var(--well)]" : "border-[var(--hair)]"
            }`}
          >
            <input
              type="radio"
              name="marketplace-action"
              className="mt-[3px]"
              checked={action === entry.id}
              disabled={busy}
              onChange={() => onChange({ action: entry.id })}
            />
            <span className="flex flex-col">
              <span className="text-[12.5px] font-medium text-[var(--silkscreen)]">{entry.label}</span>
              <span className="text-[11px] text-[var(--mute)]">{entry.hint}</span>
            </span>
          </label>
        ))}
      </fieldset>

      {local ? (
        <>
          <div className="flex flex-col gap-2 rounded-lg border border-[var(--hair)] p-2.5">
            {action === "selected" ? (
              <input
                type="search"
                className="w-full rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 py-1.5 text-[12px] text-[var(--silkscreen)]"
                aria-label="Filter marketplace items"
                placeholder="filter skills and agents…"
                value={filter}
                onChange={(event) => onChange({ filter: event.target.value })}
              />
            ) : null}
            <div className="sheet-tree flex flex-col gap-2.5 pr-1">
              {inspect.plugins.map((plugin) => (
                <PluginGroup
                  key={plugin.name}
                  plugin={plugin}
                  readOnly={action === "all"}
                  keys={keys}
                  filter={filter}
                  canAgents={canAgents}
                  busy={busy}
                  onKeysChange={(next) => onChange({ keys: next })}
                />
              ))}
            </div>
          </div>
          <ProviderChips
            visibleAgents={visibleAgents}
            providers={providers}
            onChange={(next) => onChange({ providers: next })}
            disabled={busy}
            warning={wantsAgents && !canAgents ? copy.agentsNeedClaude : null}
          />
          <ScopePicker
            scope={scope}
            onChange={(next) => onChange({ scope: next })}
            projects={projects}
            providers={providers}
            onPickFolder={onPickFolder}
            disabled={busy}
          />
          {result ? <OutcomeList result={result} busy={busy} onOverwriteConflicts={onOverwriteConflicts} /> : null}
        </>
      ) : null}
    </div>
  );
}

type PluginGroupProps = {
  plugin: MarketplacePlugin;
  readOnly: boolean;
  keys: ReadonlySet<string>;
  filter: string;
  canAgents: boolean;
  busy: boolean;
  onKeysChange: (keys: Set<string>) => void;
};

function PluginGroup({ plugin, readOnly, keys, filter, canAgents, busy, onKeysChange }: PluginGroupProps) {
  const query = filter.trim().toLowerCase();
  const matches = (entry: MarketplaceEntry) =>
    !query || entry.name.toLowerCase().includes(query) || entry.description.toLowerCase().includes(query);
  return (
    <section role="group" aria-label={plugin.name} className="flex flex-col gap-1">
      <header className="flex items-baseline gap-2">
        <span className="text-[12px] font-semibold text-[var(--silkscreen)]">{plugin.name}</span>
        {plugin.version ? (
          <span className="font-mono text-[10.5px] text-[var(--mute)]">v{plugin.version}</span>
        ) : null}
      </header>
      {!plugin.supported ? (
        <p className="text-[11px] text-[var(--mute)]">{copy.unsupportedPlugin}</p>
      ) : (
        <>
          <EntryList
            title="Skills"
            kind="skill"
            entries={plugin.skills.filter(matches)}
            plugin={plugin}
            keys={keys}
            readOnly={readOnly}
            disabled={busy}
            onKeysChange={onKeysChange}
          />
          <EntryList
            title="Subagents"
            tag={copy.claudeOnly}
            kind="agent"
            entries={plugin.agents.filter(matches)}
            plugin={plugin}
            keys={keys}
            readOnly={readOnly}
            disabled={busy || !canAgents}
            onKeysChange={onKeysChange}
          />
        </>
      )}
    </section>
  );
}

type EntryListProps = {
  title: string;
  tag?: string;
  kind: ItemKind;
  entries: MarketplaceEntry[];
  plugin: MarketplacePlugin;
  keys: ReadonlySet<string>;
  readOnly: boolean;
  disabled: boolean;
  onKeysChange: (keys: Set<string>) => void;
};

function EntryList({ title, tag, kind, entries, plugin, keys, readOnly, disabled, onKeysChange }: EntryListProps) {
  if (entries.length === 0) {
    return null;
  }
  const group = groupKeys(plugin, kind);
  const allOn = group.length > 0 && group.every((key) => keys.has(key));
  const linkClass = "underline disabled:no-underline disabled:opacity-45";
  return (
    <div className="flex flex-col gap-0.5 pl-1">
      <div className="flex items-center gap-2 text-[10.5px] tracking-[0.04em] text-[var(--mute)] uppercase">
        <span>{title}</span>
        {tag ? <span className="rounded border border-[var(--hair)] px-1 normal-case">{tag}</span> : null}
        {!readOnly ? (
          <span className="ml-auto flex gap-1 normal-case">
            <button
              type="button"
              className={linkClass}
              disabled={disabled || allOn}
              onClick={() => onKeysChange(toggleGroup(keys, plugin, kind, true))}
            >
              {copy.selectAll}
            </button>
            <span aria-hidden="true">·</span>
            <button
              type="button"
              className={linkClass}
              disabled={disabled || !group.some((key) => keys.has(key))}
              onClick={() => onKeysChange(toggleGroup(keys, plugin, kind, false))}
            >
              {copy.selectNone}
            </button>
          </span>
        ) : null}
      </div>
      <ul className="flex flex-col">
        {entries.map((entry) => {
          const key = entryKey(plugin.name, kind, entry.path);
          const checked = readOnly ? !disabled : keys.has(key);
          return (
            <li key={key}>
              <label
                className={`flex items-start gap-2 rounded px-1 py-[3px] text-[12px] ${
                  disabled ? "opacity-50" : "hover:bg-[var(--well)]"
                }`}
              >
                <input
                  type="checkbox"
                  className="mt-[3px]"
                  aria-label={entry.name}
                  checked={checked}
                  disabled={disabled || readOnly}
                  onChange={(event) => {
                    const next = new Set(keys);
                    if (event.target.checked) {
                      next.add(key);
                    } else {
                      next.delete(key);
                    }
                    onKeysChange(next);
                  }}
                />
                <span className="flex min-w-0 flex-col">
                  <span className="font-medium text-[var(--silkscreen)]">{entry.name}</span>
                  {entry.description ? (
                    <span className="truncate text-[11px] text-[var(--mute)]" title={entry.description}>
                      {entry.description}
                    </span>
                  ) : null}
                </span>
              </label>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function OutcomeList({
  result,
  busy,
  onOverwriteConflicts,
}: {
  result: InstallItemsResult;
  busy: boolean;
  onOverwriteConflicts: () => void;
}) {
  const summary = summarizeOutcomes(result);
  const rows = result.outcomes.filter((outcome) => outcome.status !== "installed" && outcome.status !== "replaced");
  return (
    <div className="flex flex-col gap-1.5 rounded-lg border border-[var(--hair)] bg-[var(--well)] p-2.5" role="status">
      <p className="text-[12px] text-[var(--silkscreen)]">
        {copy.itemsDone(summary.installed, summary.touchedProviders.length)}
        {result.shaMoved ? <span className="text-[var(--mute)]"> {copy.shaMoved}</span> : null}
      </p>
      {rows.length > 0 ? (
        <ul className="flex flex-col gap-0.5 font-mono text-[11px]">
          {rows.map((outcome) => (
            <li key={`${outcome.provider}:${entryKey(outcome.pluginName, outcome.kind, outcome.path)}`} className="flex gap-2">
              <span className={outcomeTone(outcome)}>{outcomeLabel(outcome)}</span>
              <span className="text-[var(--silkscreen)]">
                {outcome.name} · {outcome.provider}
              </span>
              {outcome.reason ? <span className="truncate text-[var(--mute)]">{outcome.reason}</span> : null}
            </li>
          ))}
        </ul>
      ) : null}
      {summary.conflicts.length > 0 ? (
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="h-7 rounded-md border border-[var(--warn)] px-2.5 text-[11.5px] font-semibold text-[var(--warn)] disabled:opacity-45"
            disabled={busy}
            onClick={onOverwriteConflicts}
          >
            {copy.overwriteConflicts(summary.conflicts.length)}
          </button>
          <span className="text-[11px] text-[var(--mute)]">{copy.conflictHint}</span>
        </div>
      ) : null}
    </div>
  );
}

function outcomeLabel(outcome: ItemOutcome): string {
  switch (outcome.status) {
    case "installed":
      return copy.itemInstalled;
    case "replaced":
      return copy.itemReplaced;
    case "skipped":
      return copy.itemSkipped;
    case "conflict":
      return copy.itemConflict;
    case "failed":
      return copy.itemFailed;
  }
}

function outcomeTone(outcome: ItemOutcome): string {
  switch (outcome.status) {
    case "conflict":
      return "text-[var(--warn)]";
    case "failed":
      return "text-[var(--trip)]";
    default:
      return "text-[var(--mute)]";
  }
}
