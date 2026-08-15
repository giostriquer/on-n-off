export type StartupMark =
  | "selected-local-ready"
  | "remembered-scope-ready"
  | "usage-start"
  | "updater-check-start"
  | "background-providers-start";

export function markStartup(name: StartupMark): void {
  if (typeof performance !== "undefined" && typeof performance.mark === "function") {
    performance.mark(`on-n-off:${name}`);
  }
}
