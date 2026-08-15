import {
  catalogCounts,
  driftRows,
  liveRows,
  masterAllOn,
  type CatalogCounts,
  type DriftRow,
  type LiveRow,
} from "$lib/catalog";
import { filterTab, type FilteredTab } from "$lib/filterTab";
import { mergeProjects, projectLabel, sameProjectPath } from "$lib/project";
import type { AgentTabDto, ProjectDto } from "$lib/types";

export type CatalogInventory = {
  counts: CatalogCounts;
  live: LiveRow[];
  drift: DriftRow[];
  allOn: boolean;
};

export type CatalogFilterView = {
  filtered: FilteredTab | null;
  live: LiveRow[];
};

export type ProjectView = {
  projects: ProjectDto[];
  path: string | null;
  label: string;
  note: string;
};

export function deriveCatalogInventory(dto: AgentTabDto | null): CatalogInventory {
  return {
    counts: catalogCounts(dto),
    live: dto ? liveRows(dto) : [],
    drift: dto ? driftRows(dto) : [],
    allOn: masterAllOn(dto),
  };
}

export function deriveCatalogFilterView(
  dto: AgentTabDto | null,
  filter: string,
  live: LiveRow[],
): CatalogFilterView {
  const query = filter.trim().toLowerCase();
  return {
    filtered: dto ? filterTab(dto, filter) : null,
    live: live.filter((row) => !query || `${row.name} ${row.meta} ${row.id}`.toLowerCase().includes(query)),
  };
}

export function deriveProjectView(
  projects: ProjectDto[],
  extraProjects: ProjectDto[],
  path: string | null,
): ProjectView {
  const merged = mergeProjects(projects, extraProjects);
  if (!path) {
    return {
      projects: merged,
      path,
      label: "all projects",
      note: "global agent config is the source of truth",
    };
  }
  return {
    projects: merged,
    path,
    label: merged.find((project) => sameProjectPath(project.path, path))?.label ?? projectLabel(path),
    note: `local skills · ${path}`,
  };
}
