import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { appPolicy, cspHeader } from "./csp.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const policy = appPolicy(root);
const IPC_BRIDGE = ["ipc:", "http://ipc.localhost"];

test("the webview runs under exactly this content security policy", () => {
  assert.deepEqual(policy, {
    "default-src": ["'self'"],
    "script-src": ["'self'"],
    "style-src": ["'self'"],
    "style-src-attr": ["'unsafe-inline'"],
    "img-src": ["'self'"],
    "font-src": ["'self'"],
    "connect-src": IPC_BRIDGE,
    "object-src": ["'none'"],
    "base-uri": ["'none'"],
    "form-action": ["'none'"],
    "frame-ancestors": ["'none'"],
  });
});

test("every source is a keyword or the Tauri IPC bridge: no host, scheme, data: or blob: source", () => {
  for (const [directive, sources] of Object.entries(policy)) {
    for (const source of sources) {
      assert.ok(/^'[a-z-]+'$/.test(source) || IPC_BRIDGE.includes(source), `${directive} ${source}`);
      assert.ok(!/unsafe-eval/.test(source), `${directive} ${source}`);
    }
  }
  assert.ok(!Object.keys(policy).some((directive) => /^script-src-/.test(directive)), "no script-src-elem or -attr widens script-src");
});

test("index.html loads its script from a file and carries no inline handler, style or style element", () => {
  const html = readFileSync(join(root, "ui", "index.html"), "utf8");
  assert.doesNotMatch(html, /\sstyle\s*=/i);
  assert.doesNotMatch(html, /\son[a-z]+\s*=/i);
  for (const script of html.match(/<script\b[^>]*>/gi) ?? []) assert.match(script, /\ssrc\s*=/i, script);
  assert.doesNotMatch(html, /<style\b/i);
});

test("no UI source renders a style element or injects raw HTML", () => {
  const files = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "ui/src/*.tsx", "ui/src/*.ts"], { cwd: root, encoding: "utf8" })
    .split("\0")
    .filter((file) => file && !/\.test\.tsx?$/.test(file) && !file.startsWith("ui/src/dev/"));
  assert.ok(files.includes("ui/src/main.tsx"), "the scan reaches the UI sources");
  for (const file of files) {
    const source = readFileSync(join(root, file), "utf8");
    assert.doesNotMatch(source, /<style\b/, file);
    assert.doesNotMatch(source, /dangerouslySetInnerHTML/, file);
  }
});

test("the header form joins each directive's sources in order", () => {
  assert.equal(
    cspHeader({ "default-src": ["'self'"], "connect-src": ["ipc:", "http://ipc.localhost"] }),
    "default-src 'self'; connect-src ipc: http://ipc.localhost",
  );
});
