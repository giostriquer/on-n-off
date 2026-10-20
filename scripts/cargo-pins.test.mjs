import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const skip = globalThis.Bun?.TOML ? false : "needs Bun's TOML parser";
const SINGLE_COPY = {
  tao: "move the cfg(windows) `tao` pin in src-tauri/Cargo.toml to tauri-runtime-wry's",
  windows: "move the cfg(windows) `windows` pin in src-tauri/Cargo.toml to tauri-runtime's, then `cargo update -p` whichever crate still holds the old one",
  "windows-core": "update the crate that still pulls the older `windows`",
  winreg: "move the cfg(windows) `winreg` pin in src-tauri/Cargo.toml to embed-resource's",
};

test("the Windows build compiles one copy each of tao, windows and winreg, the largest codegen units in its tree", { skip }, () => {
  const packages = globalThis.Bun.TOML.parse(readFileSync(join(root, "src-tauri", "Cargo.lock"), "utf8")).package;
  for (const [name, fix] of Object.entries(SINGLE_COPY)) {
    const versions = packages.filter((candidate) => candidate.name === name).map((candidate) => candidate.version);
    assert.equal(versions.length, 1, `Cargo.lock holds ${name} ${versions.join(" and ")}: ${fix}`);
  }
});
