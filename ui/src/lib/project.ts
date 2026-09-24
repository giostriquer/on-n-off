import type { ProjectDto } from "./types";

export function isProjectOrigin(origin?: string): boolean {
  return origin?.toLowerCase() === "project";
}

/** A server an enabled plugin brings: live wherever the plugin is. */
export function isPluginOrigin(origin?: string): boolean {
  return origin?.toLowerCase() === "plugin";
}

/**
 * A server Claude keeps for particular projects, listed in the all-projects view: it runs only
 * inside those projects, so it is not live here.
 */
export function isLocalOrigin(origin?: string): boolean {
  return origin?.toLowerCase() === "local";
}

export function normalizeProjectKey(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

export function sameProjectPath(a: string, b: string): boolean {
  return normalizeProjectKey(a) === normalizeProjectKey(b);
}

export function projectLabel(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

export function projectFromPath(path: string): ProjectDto {
  return {
    id: normalizeProjectKey(path),
    label: projectLabel(path),
    path,
    branch: "",
    skillCount: 0,
    mcpCount: 0,
  };
}

export function looksLikeFolderPath(value: string): boolean {
  const trimmed = value.trim();
  if (trimmed.length < 2) {
    return false;
  }
  return /^(~[\\/]?|\/|\.\/|\.\.[\\/]|[A-Za-z]:[\\/])/.test(trimmed) || /[\\/]/.test(trimmed);
}

export function mergeProjects(recognized: ProjectDto[], extra: ProjectDto[]): ProjectDto[] {
  const out: ProjectDto[] = [];
  for (const project of [...recognized, ...extra]) {
    if (out.some((item) => sameProjectPath(item.path, project.path))) {
      continue;
    }
    out.push(project);
  }
  return out.sort(
    (a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "accent" }) || a.path.localeCompare(b.path),
  );
}
