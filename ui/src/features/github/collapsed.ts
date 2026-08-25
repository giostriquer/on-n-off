/** Which Pull requests sections the user folded, remembered across launches in this browser. */

const KEY = "on-n-off.github.collapsed";

export type SectionId = "mine" | "reviewRequested" | "assigned";

export function readCollapsed(): Set<SectionId> {
  try {
    const raw = localStorage.getItem(KEY);
    const ids = raw ? (JSON.parse(raw) as unknown) : [];
    return new Set(Array.isArray(ids) ? (ids.filter((id) => typeof id === "string") as SectionId[]) : []);
  } catch {
    return new Set();
  }
}

export function writeCollapsed(ids: Set<SectionId>) {
  try {
    localStorage.setItem(KEY, JSON.stringify([...ids]));
  } catch {
    // Storage can be unavailable (private mode, quota); the fold then lasts for the session.
  }
}
