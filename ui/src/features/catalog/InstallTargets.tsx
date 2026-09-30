import { Segmented } from "@/components/Segmented";
import { copy } from "$lib/copy";
import { previewPath } from "$lib/marketplaceSelection";
import type { AgentId, AgentInfo, ItemScope, ProjectDto } from "$lib/types";

const LABEL = "text-[10px] font-semibold tracking-[0.05em] text-[var(--mute)]";

/** Which providers receive the items. */
export function ProviderChips({
  visibleAgents,
  providers,
  onChange,
  disabled,
  warning,
}: {
  visibleAgents: AgentInfo[];
  providers: readonly AgentId[];
  onChange: (providers: AgentId[]) => void;
  disabled: boolean;
  warning: string | null;
}) {
  function toggle(id: AgentId) {
    onChange(providers.includes(id) ? providers.filter((p) => p !== id) : [...providers, id]);
  }
  return (
    <div className="flex flex-col gap-1.5">
      <span className={LABEL}>{copy.targetsProviders}</span>
      <div className="flex">
        <Segmented
          ariaLabel={copy.targetsProviders}
          options={visibleAgents.map((agent) => ({ value: agent.id, label: agent.displayName }))}
          pressed={(id) => providers.includes(id)}
          onPress={toggle}
          disabled={disabled}
        />
      </div>
      {warning ? <p className="text-[11px] text-[var(--warn)]">{warning}</p> : null}
    </div>
  );
}

/** Global vs. one project, with the resolved skill folder per provider underneath. */
export function ScopePicker({
  scope,
  onChange,
  projects,
  providers,
  onPickFolder,
  disabled,
}: {
  scope: ItemScope;
  onChange: (scope: ItemScope) => void;
  projects: ProjectDto[];
  providers: readonly AgentId[];
  onPickFolder: () => Promise<string | null>;
  disabled: boolean;
}) {
  function pickFolder() {
    void onPickFolder().then((dir) => {
      if (dir) {
        onChange({ kind: "project", projectPath: dir });
      }
    });
  }

  return (
    <div className="flex flex-col gap-1.5">
      <span className={LABEL}>{copy.targetsScope}</span>
      <div className="flex items-center gap-1.5">
        <Segmented
          ariaLabel="Install scope"
          options={[
            { value: "global", label: copy.scopeGlobal },
            { value: "project", label: copy.scopeProject },
          ]}
          pressed={(kind) => kind === scope.kind}
          onPress={(kind) => {
            if (kind === scope.kind) return;
            if (kind === "global") {
              onChange({ kind: "global" });
              return;
            }
            const first = projects[0]?.path;
            if (first) {
              onChange({ kind: "project", projectPath: first });
            } else {
              pickFolder();
            }
          }}
          disabled={disabled}
        />
        {scope.kind === "project" ? (
          <>
            <select
              className="h-7 min-w-0 flex-1 rounded-md border border-[var(--hair)] bg-[var(--well)] px-1.5 font-mono text-[12px] text-[var(--silkscreen)]"
              aria-label="Project"
              value={scope.projectPath}
              disabled={disabled}
              onChange={(event) => onChange({ kind: "project", projectPath: event.target.value })}
            >
              {projects.some((project) => project.path === scope.projectPath) ? null : (
                <option value={scope.projectPath}>{scope.projectPath}</option>
              )}
              {projects.map((project) => (
                <option key={project.id} value={project.path}>
                  {project.label} — {project.path}
                </option>
              ))}
            </select>
            <button
              type="button"
              className="h-7 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 text-[12px] text-[var(--silkscreen)]"
              disabled={disabled}
              onClick={pickFolder}
            >
              {copy.folder}
            </button>
          </>
        ) : null}
      </div>
      {providers.length > 0 ? (
        <ul className="flex flex-col gap-0.5 font-mono text-[11px] text-[var(--mute)]">
          {providers.map((provider) => (
            <li key={provider}>{previewPath(provider, scope)}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
