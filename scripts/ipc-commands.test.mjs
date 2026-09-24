// Run with `bun test scripts/` (CI's frontend job). The UI reaches Rust only through
// `ui/src/lib/api.ts`, by command name; a name the Rust side does not register fails only at run
// time, on the one screen that calls it. These tests hold the two lists together.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");

/** `source` with its line and block comments blanked and its strings kept, so a comment naming a
 * command neither counts as a call nor hides a missing one. */
function withoutComments(source) {
  let out = "";
  let quote = null;
  for (let i = 0; i < source.length; i += 1) {
    const char = source[i];
    if (quote) {
      out += char;
      if (char === "\\") out += source[++i] ?? "";
      else if (char === quote) quote = null;
    } else if (char === '"' || char === "'" || char === "`") {
      quote = char;
      out += char;
    } else if (source.startsWith("//", i)) {
      while (i < source.length && source[i] !== "\n") i += 1;
      out += "\n";
    } else if (source.startsWith("/*", i)) {
      const end = source.indexOf("*/", i + 2);
      i = end === -1 ? source.length : end + 1;
      out += " ";
    } else {
      out += char;
    }
  }
  return out;
}

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
  const api = withoutComments(read("ui/src/lib/api.ts"));
  // Every call, however it is typed (`invoke(`, `invoke<Array<Dto>>(`): anything not read as a
  // literal name below then fails, rather than slipping past every check.
  const calls = [...api.matchAll(/\binvoke\s*[<(]/g)].length;
  const names = [...api.matchAll(/\binvoke\s*(?:<[^(]*>)?\(\s*"([^"]+)"/g)].map((match) => match[1]);
  assert.equal(names.length, calls, "api.ts invokes a command whose name is not a string literal");
  return names;
}

/** The app's commands the dev mock (`?mock`) answers. */
function mockedCommands() {
  const mock = withoutComments(read("ui/src/dev/mockIpc.ts"));
  const start = mock.indexOf("const handlers: Record<string, Handler> = {");
  assert.notEqual(start, -1, "mockIpc.ts has no handlers table");
  const end = mock.indexOf("\n};\n", start);
  assert.notEqual(end, -1, "mockIpc.ts's handlers table has no end");
  // `name: (args) => …`, `"name": …` and `name(args) { … }` alike.
  const keys = [...mock.slice(start, end).matchAll(/^ {2}(?:"([^"]+)"|(\w+))\s*[:(]/gm)].map(
    (match) => match[1] ?? match[2],
  );
  // Tauri's own plugins answer `plugin:<name>|<command>`; they are not the app's to register.
  return keys.filter((key) => !key.includes(":") && !key.includes("|"));
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

/** Every UI source file, tests and the dev mock aside. */
function uiSources(directory = join(root, "ui", "src")) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return entry.name === "dev" ? [] : uiSources(path);
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

test("no UI file but api.ts reaches Tauri's invoke", () => {
  const callers = uiSources()
    .filter((path) => /@tauri-apps\/api\/core/.test(withoutComments(readFileSync(path, "utf8"))))
    .map((path) => relative(root, path).split("\\").join("/"));
  assert.deepEqual(callers, ["ui/src/lib/api.ts"]);
});
