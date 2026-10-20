import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const skip = globalThis.Bun?.TOML ? false : "needs Bun's TOML parser";

function versionUsedBy(packages, owner, dependency) {
  const entry = packages.find((candidate) => candidate.name === owner)?.dependencies?.find((name) => name.split(" ")[0] === dependency);
  assert.ok(entry, `${owner} does not depend on ${dependency}`);
  const pinned = entry.split(" ")[1];
  if (pinned) return pinned;
  const versions = packages.filter((candidate) => candidate.name === dependency).map((candidate) => candidate.version);
  assert.equal(versions.length, 1, `${dependency} has several versions but ${owner} names none`);
  return versions[0];
}

test("the app's own tao and windows are the versions the Tauri runtime uses, so a Windows build compiles one copy of each", { skip }, () => {
  const packages = globalThis.Bun.TOML.parse(readFileSync(join(root, "src-tauri", "Cargo.lock"), "utf8")).package;
  assert.equal(versionUsedBy(packages, "on-n-off", "tao"), versionUsedBy(packages, "tauri-runtime-wry", "tao"));
  assert.equal(versionUsedBy(packages, "on-n-off", "windows"), versionUsedBy(packages, "tauri-runtime", "windows"));
});
