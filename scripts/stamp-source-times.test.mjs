// Run with `bun test scripts/` (CI's frontend job) or `node --test scripts/*.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { contentTime, stampSourceTimes } from "./stamp-source-times.mjs";

const git = (cwd, ...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const mtime = (path) => statSync(path).mtimeMs / 1000;

function write(root, path, text) {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), text);
}

/** A throwaway repository with one committed Swift package and a file outside it. */
function repository(t) {
  const root = mkdtempSync(join(tmpdir(), "on-n-off-stamp-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  git(root, "init", "-q");
  write(root, "macos/Notch/Package.swift", "// swift-tools-version: 5.9\n");
  write(root, "macos/Notch/Sources/Core/Meter.swift", "let meter = 1\n");
  write(root, "macos/Notch/Sources/Core/Ink.swift", "let ink = 2\n");
  write(root, "README.md", "outside the packages\n");
  git(root, "add", "-A");
  git(root, "-c", "user.name=me", "-c", "user.email=you@example.com", "commit", "-qm", "fixture");
  return root;
}

/** A second checkout of the same commit, as the next CI run gets: same content, new files. */
function checkoutAgain(t, root) {
  const copy = mkdtempSync(join(tmpdir(), "on-n-off-stamp-copy-"));
  t.after(() => rmSync(copy, { recursive: true, force: true }));
  git(copy, "clone", "-q", root, ".");
  return copy;
}

test("the time depends on the content alone, and stays in the past", () => {
  const time = contentTime(Buffer.from("let meter = 1\n"));
  assert.equal(time, contentTime(Buffer.from("let meter = 1\n")));
  assert.notEqual(time, contentTime(Buffer.from("let meter = 2\n")));
  assert.ok(Number.isInteger(time), "whole seconds survive every file system and archive format");
  assert.ok(time >= Date.UTC(2000, 0, 1) / 1000);
  assert.ok(time < Date.UTC(2009, 0, 1) / 1000, "never a future time, which build tools warn about");
});

test("two checkouts of the same commit get the same times", (t) => {
  const first = repository(t);
  const second = checkoutAgain(t, first);
  // A checkout stamps every file with the time it was written, which differs between runs.
  utimesSync(join(second, "macos/Notch/Sources/Core/Meter.swift"), 1_900_000_000, 1_900_000_000);
  assert.equal(stampSourceTimes(first, ["macos"]), 3);
  assert.equal(stampSourceTimes(second, ["macos"]), 3);
  for (const path of ["macos/Notch/Package.swift", "macos/Notch/Sources/Core/Meter.swift", "macos/Notch/Sources/Core/Ink.swift"]) {
    assert.equal(mtime(join(second, path)), mtime(join(first, path)), path);
    assert.equal(mtime(join(first, path)), contentTime(Buffer.from(git(first, "show", `HEAD:${path}`))), path);
  }
  assert.notEqual(mtime(join(first, "macos/Notch/Sources/Core/Meter.swift")), mtime(join(first, "macos/Notch/Sources/Core/Ink.swift")));
});

// A build tool compares a source's time with the one it recorded. A changed file must not keep the
// time of the content the cached build was made from, or the build would reuse a stale object.
test("a changed file gets another time, even one older than its outputs", (t) => {
  const root = repository(t);
  stampSourceTimes(root, ["macos"]);
  const meter = join(root, "macos/Notch/Sources/Core/Meter.swift");
  const before = mtime(meter);
  write(root, "macos/Notch/Sources/Core/Meter.swift", "let meter = 3\n");
  stampSourceTimes(root, ["macos"]);
  assert.notEqual(mtime(meter), before);
  assert.equal(mtime(meter), contentTime(Buffer.from("let meter = 3\n")), "the file on disk decides, not the commit");
});

test("only tracked files under the given paths change", (t) => {
  const root = repository(t);
  write(root, "macos/Notch/.build/debug/Meter.o", "object");
  const untouched = [join(root, "README.md"), join(root, "macos/Notch/.build/debug/Meter.o")];
  for (const path of untouched) utimesSync(path, 1_800_000_000, 1_800_000_000);
  stampSourceTimes(root, ["macos"]);
  for (const path of untouched) assert.equal(mtime(path), 1_800_000_000, path);
});

test("a path with no tracked files is an error, not a silent no-op", (t) => {
  const root = repository(t);
  assert.throws(() => stampSourceTimes(root, ["missing"]), /No tracked files under missing/);
});
