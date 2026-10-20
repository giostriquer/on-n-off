import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, extname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE_PATTERNS = ["*.rs", "*.ts", "*.tsx", "*.swift", "*.mjs"];

const COMMENT = /^\s*(\/\/|\/\*|\{\s*\/\*)/;
const DIRECTIVE = /^\s*(\/\/\/ <reference .*\/>\s*$|\/\/ @ts-expect-error\s*$|\/\/ swift-tools-version:)/;

function sources() {
  return execFileSync(
    "git",
    ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", ...SOURCE_PATTERNS],
    { cwd: root, encoding: "utf8" },
  )
    .split("\0")
    .filter((file) => file && existsSync(join(root, file)));
}

function commentLines(file) {
  const lines = readFileSync(join(root, file), "utf8").split("\n");
  return lines.flatMap((line, index) => {
    if (!COMMENT.test(line) || DIRECTIVE.test(line)) return [];
    return [`${file}:${index + 1}: ${line.trim()}`];
  });
}

test("the scan reaches nested sources in every language and skips build output", () => {
  const scanned = sources();
  for (const extension of [".rs", ".ts", ".tsx", ".swift", ".mjs"]) {
    assert.ok(
      scanned.some((file) => extname(file) === extension && file.includes("/")),
      `no nested ${extension} source was scanned`,
    );
  }
  assert.ok(!scanned.some((file) => /(^|\/)(node_modules|\.build)\//.test(file)));
});

test("sources carry no code comments", () => {
  const found = sources().flatMap(commentLines);
  assert.deepEqual(found, [], "move each comment's rationale into the docs that own the subject");
});

test("the comment pattern sees line, block, doc and JSX comments, not URLs or directives", () => {
  for (const line of ["// note", "  /// doc", "//! crate", "/* block */", "  {/* jsx */}"]) {
    assert.ok(COMMENT.test(line), line);
  }
  for (const line of ['let url = "https://example.com";', "x / y"]) {
    assert.ok(!COMMENT.test(line), line);
  }
  for (const line of ['/// <reference types="vite/client" />', "// @ts-expect-error", "// swift-tools-version: 5.9"]) {
    assert.ok(DIRECTIVE.test(line), line);
  }
  for (const line of ["// @ts-expect-error because the types lag", "/// <reference prose"]) {
    assert.ok(!DIRECTIVE.test(line), `prose rides no directive: ${line}`);
  }
});
