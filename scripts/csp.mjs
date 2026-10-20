import { readFileSync } from "node:fs";
import { join } from "node:path";

export function appPolicy(root) {
  const policy = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8")).app.security.csp;
  if (policy === null || typeof policy !== "object") return null;
  return Object.fromEntries(
    Object.entries(policy).map(([directive, value]) => [directive, [value].flat().flatMap((source) => String(source).split(/\s+/)).filter(Boolean)]),
  );
}

export function cspHeader(policy) {
  return Object.entries(policy)
    .map(([directive, sources]) => [directive, ...sources].join(" "))
    .join("; ");
}
