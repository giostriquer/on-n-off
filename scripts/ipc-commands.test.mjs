// Run with `bun test scripts/` (CI's frontend job). The UI reaches Rust only through
// `ui/src/lib/api.ts`, by command name; a name the Rust side does not register fails only at run
// time, on the one screen that calls it. These tests hold the two lists together.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");

/** The commands `lib.rs` hands to `tauri::generate_handler!`. */
function registeredCommands() {
  const lib = read("src-tauri/src/lib.rs");
  const list = lib.match(/generate_handler!\[([\s\S]*?)\]/);
  assert.ok(list, "lib.rs registers no commands with generate_handler!");
  const entries = list[1].split(",").map((entry) => entry.trim()).filter(Boolean);
  for (const entry of entries) {
    assert.match(entry, /^commands::\w+$/, `unexpected handler entry "${entry}"`);
  }
  return entries.map((entry) => entry.slice("commands::".length));
}

/** Every command `api.ts` invokes, in order. */
function invokedCommands() {
  const api = read("ui/src/lib/api.ts");
  const calls = [...api.matchAll(/\binvoke(?:<[^>(]*>)?\(/g)].length;
  const names = [...api.matchAll(/\binvoke(?:<[^>(]*>)?\(\s*"([^"]+)"/g)].map((match) => match[1]);
  // A name built at run time would slip past every check below.
  assert.equal(names.length, calls, "api.ts invokes a command whose name is not a string literal");
  return names;
}

/** The commands the dev mock (`?mock`) answers. */
function mockedCommands() {
  const mock = read("ui/src/dev/mockIpc.ts");
  const start = mock.indexOf("const handlers: Record<string, Handler> = {");
  assert.notEqual(start, -1, "mockIpc.ts has no handlers table");
  const table = mock.slice(start, mock.indexOf("\n};\n", start));
  return [...table.matchAll(/^ {2}(\w+):/gm)].map((match) => match[1]);
}

test("every command the UI invokes is registered on the Rust side", () => {
  const registered = new Set(registeredCommands());
  const missing = invokedCommands().filter((name) => !registered.has(name));
  assert.deepEqual(missing, [], "api.ts invokes commands lib.rs does not register");
});

test("every registered command is invoked from api.ts, the one place the UI calls Rust", () => {
  const invoked = new Set(invokedCommands());
  const unused = registeredCommands().filter((name) => !invoked.has(name));
  assert.deepEqual(unused, [], "lib.rs registers commands api.ts never invokes");
});

test("the dev mock answers only commands that exist", () => {
  const registered = new Set(registeredCommands());
  const stale = mockedCommands().filter((name) => !registered.has(name));
  assert.deepEqual(stale, [], "mockIpc.ts answers commands lib.rs does not register");
});
