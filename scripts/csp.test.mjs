import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { cspHeader } from "./csp.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const csp = conf.app.security.csp;
const sources = (directive) => [csp[directive] ?? []].flat().flatMap((value) => String(value).split(/\s+/)).filter(Boolean);

test("the webview runs under a content security policy", () => {
  assert.equal(typeof csp, "object");
  assert.notEqual(csp, null);
});

test("scripts come only from the app bundle: no inline script, no eval, no remote host", () => {
  assert.deepEqual(sources("default-src"), ["'self'"]);
  assert.deepEqual(sources("script-src"), ["'self'"]);
});

test("stylesheets come only from the app bundle; only style attributes may be inline", () => {
  assert.deepEqual(sources("style-src"), ["'self'"]);
  assert.deepEqual(sources("style-src-attr"), ["'unsafe-inline'"]);
  assert.equal(csp["style-src-elem"], undefined);
});

test("the page talks only to the Tauri IPC bridge", () => {
  assert.deepEqual(sources("connect-src"), ["ipc:", "http://ipc.localhost"]);
});

test("plugins, base rewrites, form posts and framing are refused", () => {
  for (const directive of ["object-src", "base-uri", "form-action", "frame-ancestors"]) {
    assert.deepEqual(sources(directive), ["'none'"], directive);
  }
});

test("no directive allows eval or a remote host", () => {
  for (const directive of Object.keys(csp)) {
    for (const source of sources(directive)) {
      assert.ok(!/unsafe-eval|wasm-unsafe-eval/.test(source), `${directive} ${source}`);
      assert.ok(!/^(https?:|wss?:|\*)/.test(source) || source === "http://ipc.localhost", `${directive} ${source}`);
    }
  }
});

test("index.html carries no inline style, script or handler that the policy would refuse", () => {
  const html = readFileSync(join(root, "ui", "index.html"), "utf8");
  assert.doesNotMatch(html, /\sstyle\s*=/i);
  assert.doesNotMatch(html, /\son[a-z]+\s*=/i);
  for (const script of html.match(/<script\b[^>]*>/gi) ?? []) assert.match(script, /\ssrc\s*=/i, script);
  assert.doesNotMatch(html, /<style\b/i);
});

test("the header form joins each directive's sources in order", () => {
  assert.equal(
    cspHeader({ "default-src": "'self'", "connect-src": ["ipc:", "http://ipc.localhost"] }),
    "default-src 'self'; connect-src ipc: http://ipc.localhost",
  );
});
