import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, extname, join, relative } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const SOURCES = [
  { dir: "src-tauri/src", extensions: [".rs"] },
  { dir: "ui/src", extensions: [".ts", ".tsx"] },
  { dir: "src-tauri/macos", extensions: [".swift"], skip: ["/.build/"] },
];

const COMMENT = /^\s*(\/\/|\/\*|\{\s*\/\*)/;
const DIRECTIVE = /^\s*(\/\/\/ <reference |\/\/ @ts-expect-error\b|\/\/ swift-tools-version:)/;

function files(dir, extensions, skip = []) {
  const found = [];
  const pending = [join(root, dir)];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const path = join(current, entry.name);
      if (skip.some((part) => `${path}/`.includes(part))) continue;
      if (entry.isDirectory()) pending.push(path);
      else if (extensions.includes(extname(entry.name))) found.push(path);
    }
  }
  return found;
}

function commentLines(path) {
  const lines = readFileSync(path, "utf8").split("\n");
  return lines.flatMap((line, index) => {
    if (!COMMENT.test(line) || DIRECTIVE.test(line)) return [];
    return [`${relative(root, path)}:${index + 1}: ${line.trim()}`];
  });
}

test("app sources carry no code comments", () => {
  const found = SOURCES.flatMap(({ dir, extensions, skip }) =>
    files(dir, extensions, skip).flatMap(commentLines),
  );
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
  assert.ok(!DIRECTIVE.test("// @ts-expect-errors are fine here"), "a prose comment is not a directive");
});
