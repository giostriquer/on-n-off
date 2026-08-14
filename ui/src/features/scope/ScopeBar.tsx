import { useEffect, useMemo, useRef, useState } from "react";
import { scopeConfigPath } from "$lib/catalog";
import { looksLikeFolderPath, projectLabel, sameProjectPath, scopeChip } from "$lib/project";
import type { AgentId, ProjectDto } from "$lib/types";
import "./ScopeBar.css";

type ScopeBarProps = {
  agentId: AgentId;
  projects: ProjectDto[];
  selectedPath: string | null;
  note: string;
  tally: string;
  globalItems: number;
  onSelect: (path: string | null) => void;
  onPickFolder: () => void;
  onOpenPath: (path: string) => void;
};

function FolderIcon({ on }: { on: boolean }) {
  return <span className={`scope-folder${on ? " is-on" : ""}`} aria-hidden="true" />;
}

export function ScopeBar({
  agentId,
  projects,
  selectedPath,
  note,
  tally,
  globalItems,
  onSelect,
  onPickFolder,
  onOpenPath,
}: ScopeBarProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const barRef = useRef<HTMLDivElement>(null);

  const selected = selectedPath
    ? (projects.find((project) => sameProjectPath(project.path, selectedPath)) ?? null)
    : null;
  const title = selected?.label ?? "All projects";
  const pathLine = selected?.path ?? scopeConfigPath(agentId);
  const chip = scopeChip(selected);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      return projects;
    }
    return projects.filter(
      (project) => project.label.toLowerCase().includes(q) || project.path.toLowerCase().includes(q),
    );
  }, [projects, query]);

  const pastePath = looksLikeFolderPath(query) ? query.trim() : "";
  const queryKey = query.trim().toLowerCase();
  const showAllRow = !queryKey || "all projects global".includes(queryKey);
  const showEmpty = !!queryKey && !showAllRow && filtered.length === 0 && !pastePath;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        setQuery("");
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }
    // Prefer document click-outside over a fixed transparent backdrop —
    // WebView2 often paints transparent fixed layers as an opaque grey slab.
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) {
        return;
      }
      if (barRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
      setQuery("");
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [open]);

  useEffect(() => {
    if (open) {
      searchRef.current?.focus();
    }
  }, [open]);

  function closePicker() {
    setOpen(false);
    setQuery("");
  }

  function togglePicker() {
    setOpen((prev) => {
      if (prev) {
        setQuery("");
      }
      return !prev;
    });
  }

  return (
    <div className="scope-bar" ref={barRef}>
      <span className="scope-label">SCOPE</span>

      <button
        type="button"
        className={`scope-trigger${open ? " is-open" : ""}`}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={togglePicker}
      >
        <FolderIcon on />
        <span className="scope-trigger-copy">
          <span className="scope-trigger-title">{title}</span>
          <span className="scope-trigger-path" title={pathLine}>
            {pathLine}
          </span>
        </span>
        <span className="scope-chevron">▾</span>
      </button>

      <span className={`scope-chip${selected ? " is-project" : ""}`}>{chip}</span>
      <span className="scope-note" title={note}>
        {note}
      </span>
      <span className="scope-spacer" />
      <span className="scope-tally">{tally}</span>

      {open ? (
          <div className="scope-picker" role="dialog" aria-modal="true" aria-label="Choose scope">
            <div className="scope-search">
              <span className="scope-search-icon" aria-hidden="true">
                ⌕
              </span>
              <input
                ref={searchRef}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                className="scope-search-input"
                placeholder="Search projects, or paste a folder path…"
                aria-label="Search projects, or paste a folder path"
              />
              <span className="scope-esc">esc</span>
            </div>

            <div className="scope-list">
              {showAllRow ? (
                <button
                  type="button"
                  className={`scope-row${!selected ? " is-active" : ""}`}
                  onClick={() => {
                    onSelect(null);
                    closePicker();
                  }}
                >
                  <FolderIcon on={!selected} />
                  <span className="scope-row-copy">
                    <span className="scope-row-head">
                      <span className="scope-row-name">All projects</span>
                      <span className="scope-badge">GLOBAL</span>
                    </span>
                    <span className="scope-row-path" title={scopeConfigPath(agentId)}>
                      {scopeConfigPath(agentId)}
                    </span>
                  </span>
                  <span className="scope-row-stats">{globalItems} global items</span>
                  {selected ? (
                    <span className="scope-check" aria-hidden="true" />
                  ) : (
                    <span className="scope-check is-on" aria-hidden="true">
                      ✓
                    </span>
                  )}
                </button>
              ) : null}

              {filtered.map((project) => {
                const active = !!selectedPath && sameProjectPath(project.path, selectedPath);
                return (
                  <button
                    key={project.id}
                    type="button"
                    className={`scope-row${active ? " is-active" : ""}`}
                    onClick={() => {
                      onSelect(project.path);
                      closePicker();
                    }}
                  >
                    <FolderIcon on={active} />
                    <span className="scope-row-copy">
                      <span className="scope-row-head">
                        <span className="scope-row-name">{project.label}</span>
                        {project.branch ? <span className="scope-badge">{project.branch}</span> : null}
                      </span>
                      <span className="scope-row-path" title={project.path}>
                        {project.path}
                      </span>
                    </span>
                    <span className="scope-row-stats">{`${project.skillCount ?? 0} local skills\n${project.mcpCount ?? 0} project mcps`}</span>
                    {active ? (
                      <span className="scope-check is-on" aria-hidden="true">
                        ✓
                      </span>
                    ) : (
                      <span className="scope-check" aria-hidden="true" />
                    )}
                  </button>
                );
              })}

              {pastePath ? (
                <button
                  type="button"
                  className="scope-row"
                  onClick={() => {
                    onOpenPath(pastePath);
                    closePicker();
                  }}
                >
                  <FolderIcon on={false} />
                  <span className="scope-row-copy">
                    <span className="scope-row-head">
                      <span className="scope-row-name">Open {projectLabel(pastePath)}</span>
                      <span className="scope-badge is-new">NEW</span>
                    </span>
                    <span className="scope-row-path" title={pastePath}>
                      {pastePath}
                    </span>
                  </span>
                  <span className="scope-row-stats">scan on open</span>
                  <span className="scope-check" aria-hidden="true" />
                </button>
              ) : null}

              {showEmpty ? <p className="scope-empty">No matching projects.</p> : null}
            </div>

            <div className="scope-foot">
              <button
                type="button"
                className="scope-folder-btn"
                onClick={() => {
                  closePicker();
                  onPickFolder();
                }}
              >
                Choose folder…
              </button>
              <span className="scope-foot-note">
                Reads .{agentId}/skills and .mcp.json from the folder you pick.
              </span>
            </div>
          </div>
      ) : null}
    </div>
  );
}
