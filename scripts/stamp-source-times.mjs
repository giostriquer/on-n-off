import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, readFileSync, utimesSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const epoch = Date.UTC(2000, 0, 1) / 1000;

export function contentTime(content) {
  const digest = createHash("sha256").update(content).digest("hex");
  return epoch + Number.parseInt(digest.slice(0, 7), 16);
}

export function stampSourceTimes(root, paths) {
  let stamped = 0;
  for (const path of paths) {
    const files = execFileSync("git", ["ls-files", "-z", "--", path], { cwd: root, encoding: "utf8" })
      .split("\0")
      .filter(Boolean);
    if (files.length === 0) throw new Error(`No tracked files under ${path}.`);
    for (const file of files) {
      const absolute = join(root, file);
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
