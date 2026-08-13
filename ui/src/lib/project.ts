import type { ProjectDto } from "./types";

export function isProjectOrigin(origin?: string): boolean {
  return origin?.toLowerCase() === "project";
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

export function scopeChip(project: ProjectDto | null | undefined): string {
  if (!project) {
    return "global config";
  }
  return `${project.skillCount ?? 0} local skills · ${project.mcpCount ?? 0} project mcps`;
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
