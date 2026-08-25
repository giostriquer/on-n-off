/** Which Pull requests sections the user folded, remembered across launches in this browser. */

import { GITHUB_LIST_IDS, type GithubListId } from "$lib/githubTypes";

const KEY = "on-n-off.github.collapsed";

function isListId(value: unknown): value is GithubListId {
  return typeof value === "string" && (GITHUB_LIST_IDS as readonly string[]).includes(value);
}

export function readCollapsed(): Set<GithubListId> {
  try {
    const raw = localStorage.getItem(KEY);
    const ids: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(ids) ? ids.filter(isListId) : []);
  } catch {
    return new Set();
  }
}

export function writeCollapsed(ids: Set<GithubListId>) {
  try {
    localStorage.setItem(KEY, JSON.stringify([...ids]));
  } catch {
    // Storage can be unavailable (private mode, quota); the fold then lasts for the session.
  }
}
