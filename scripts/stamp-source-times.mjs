// Gives every tracked file under the given paths a modification time derived from its content.
//
//   bun scripts/stamp-source-times.mjs src-tauri/macos
//
// The macOS jobs restore the Swift packages' .build directories from the Actions cache. SwiftPM
// decides what to recompile by comparing each source's modification time with the one it recorded,
// and a fresh checkout writes every file at the time of the checkout, so without this every run
// would recompile the packages' own modules against a cache that already held them. Stamping
// first, in the run that saves the cache and in every run that restores it, makes an unchanged
// file look unchanged, and a changed file gets another time, which is all the comparison needs.
// The time comes from the file on disk, not from git history, so it is the same in a shallow
// checkout and in a pull request's merge commit.
//
// Two contents share a time with odds of 1 in 2^28, and SwiftPM also compares sizes. Only the
// modification time changes; the content is never touched.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, readFileSync, utimesSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const epoch = Date.UTC(2000, 0, 1) / 1000;

/**
 * Whole seconds between 2000-01-01 and mid-2008, from the content's SHA-256. Always in the past,
 * because some build tools warn about a file dated in the future, and whole seconds, because every
 * file system and archive format keeps those.
 */
export function contentTime(content) {
  const digest = createHash("sha256").update(content).digest("hex");
  return epoch + Number.parseInt(digest.slice(0, 7), 16);
}

/** Stamps the tracked regular files under each path, relative to `root`; returns how many. */
export function stampSourceTimes(root, paths) {
  let stamped = 0;
  for (const path of paths) {
    const files = execFileSync("git", ["ls-files", "-z", "--", path], { cwd: root, encoding: "utf8" })
      .split("\0")
      .filter(Boolean);
    if (files.length === 0) throw new Error(`No tracked files under ${path}.`);
    for (const file of files) {
      const absolute = join(root, file);
      // A symbolic link's own time is not what a build reads, and a submodule is a directory.
      if (!lstatSync(absolute).isFile()) continue;
      const time = contentTime(readFileSync(absolute));
      utimesSync(absolute, time, time);
      stamped++;
    }
  }
  return stamped;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const paths = process.argv.slice(2);
  if (paths.length === 0) {
    console.error("Usage: bun scripts/stamp-source-times.mjs <path>...");
    process.exit(2);
  }
  const stamped = stampSourceTimes(process.cwd(), paths);
  console.log(`Stamped ${stamped} tracked files under ${paths.join(", ")} with times from their content.`);
}
