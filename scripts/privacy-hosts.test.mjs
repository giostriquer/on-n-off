import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const HOST = /https:\/\/([a-z0-9-]+(?:\.[a-z0-9-]+)+)/g;
const neverContacted = {
  "api.openai.com": "the namespace of a claim in the Codex ID token",
  "auth.meta.com": "the sign-in page Muse prints, opened in the browser",
  "cursor.com": "the install command shown for the Cursor CLI",
};

function contactedHosts() {
  const files = execFileSync("git", ["ls-files", "-z", "--", "src-tauri/src/*.rs"], { cwd: root, encoding: "utf8" })
    .split("\0")
    .filter((file) => file && !/(^|\/)tests(\.rs|\/)|fixtures\.rs$/.test(file));
  const hosts = new Set(files.flatMap((file) => [...readFileSync(join(root, file), "utf8").matchAll(HOST)].map((match) => match[1])));
  const tauri = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
  for (const endpoint of tauri.plugins.updater.endpoints) hosts.add(new URL(endpoint).hostname);
  for (const host of Object.keys(neverContacted)) hosts.delete(host);
  return [...hosts].sort();
}

function privacyHosts() {
  const readme = readFileSync(join(root, "README.md"), "utf8");
  const section = readme.split(/^## /m).find((part) => part.startsWith("Privacy\n"));
  assert.ok(section, "README has a Privacy section");
  return [...new Set([...section.matchAll(/`([a-z0-9-]+(?:\.[a-z0-9-]+)+)`/g)].map((match) => match[1]))].sort();
}

test("the README's Privacy section names exactly the hosts the app itself contacts", () => {
  assert.deepEqual(privacyHosts(), contactedHosts());
});

test("every host the scan sets aside still appears in the source, so the exclusions cannot go stale", () => {
  const source = execFileSync("git", ["grep", "-hoE", "https://[a-z0-9.-]+", "--", "src-tauri/src/*.rs"], { cwd: root, encoding: "utf8" });
  for (const host of Object.keys(neverContacted)) assert.ok(source.includes(`https://${host}`), host);
});
