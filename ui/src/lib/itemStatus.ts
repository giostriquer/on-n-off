import { copy } from "./copy";
import { isProjectOrigin } from "./project";
import type { ItemStatus, SkillDto } from "./types";

export type ItemStatusSets = { global: ItemStatus[]; project: ItemStatus[] };

/** TanStack Query key prefix for managed-item statuses: `[ITEM_STATUS_KEY, provider, path]`. */
export const ITEM_STATUS_KEY = "item-status";

/** The managed-item record behind a listed skill, if on-n-off installed it. */
export function statusForSkill(skill: SkillDto, sets: ItemStatusSets): ItemStatus | undefined {
  if (skill.pluginId) {
    return undefined;
  }
  const pool = isProjectOrigin(skill.origin) ? sets.project : sets.global;
  return pool.find(
    (status) => status.kind === "skill" && (status.displayName === skill.name || status.name === skill.name),
  );
}

export type ItemBadge = { label: string; tone: "mute" | "warn" | "trip" };

export function upstreamLabel(status: ItemStatus): string {
  if (status.upstream.state !== "updateAvailable") {
    return "";
  }
  return status.upstream.pluginVersion ? `v${status.upstream.pluginVersion}` : status.upstream.commitSha.slice(0, 7);
}

export function itemBadges(status: ItemStatus): ItemBadge[] {
  const badges: ItemBadge[] = [];
  if (status.missing) {
    badges.push({ label: copy.itemMissing, tone: "trip" });
    return badges;
  }
  if (status.installedVersion) {
    badges.push({ label: `v${status.installedVersion}`, tone: "mute" });
  }
  if (status.upstream.state === "updateAvailable") {
    badges.push({ label: copy.updateAvailable(upstreamLabel(status)), tone: "warn" });
  }
  if (status.modified) {
    badges.push({ label: copy.modifiedLocally, tone: "warn" });
  }
  return badges;
}
