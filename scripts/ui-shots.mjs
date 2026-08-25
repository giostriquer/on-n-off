#!/usr/bin/env node
// Screenshot harness: drives the UI in headless Chromium against the Vite dev server with the
// Tauri IPC mocked (`?mock=<scenario>`, see ui/src/dev/mockIpc.ts) and writes PNGs.
//
//   bun run ui:shots                      # the built-in scenes
//   bun run ui:shots scenes.json          # your own scenes (same shape as SCENES below)
//   UI_SHOTS_DIR=out bun run ui:shots     # output folder (default .tmp/ui-shots)
//
// A scene: { name, url, theme?: "dark"|"light", viewport?: {width,height}, steps?: Step[] }
// Steps run in order: {click: selector} · {fill: selector, text} · {press: key} · {hover: selector}
// · {wait: selector | {ms}} · {shot: name}. Selectors are Playwright selectors ("role=button[name=…]",
// "text=…", CSS). Every scene ends with a screenshot named after the scene unless steps took one.
// Starts its own Vite on UI_PORT (default 1425) so a running `tauri dev` on :1420 is left alone;
// set UI_BASE to point at an existing server instead.

import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { readFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright";

const PORT = Number(process.env.UI_PORT ?? 1425);
const BASE = process.env.UI_BASE ?? `http://localhost:${PORT}`;
const OUT = process.env.UI_SHOTS_DIR ?? ".tmp/ui-shots";
const VIEWPORT = { width: 1120, height: 760 };

const SCENES = [
  { name: "github-ok", url: "/github?mock=ok" },
  { name: "github-ok-light", url: "/github?mock=ok", theme: "light" },
  { name: "github-stale", url: "/github?mock=stale" },
  { name: "github-gh-missing", url: "/github?mock=ghMissing" },
  { name: "github-many", url: "/github?mock=many" },
  { name: "settings-github", url: "/settings?mock=ok", steps: [{ wait: "role=region[name='Pull requests']" }] },
];

function connects(port, host) {
  return new Promise((resolve) => {
    const socket = net.connect({ port, host });
    socket.once("connect", () => { socket.end(); resolve(true); });
    socket.once("error", () => resolve(false));
  });
}

// Vite binds "localhost", which is ::1 on some machines and 127.0.0.1 on others.
async function listening(port) {
  for (const host of ["127.0.0.1", "::1"]) {
    if (await connects(port, host)) return true;
  }
  return false;
}

async function ensureDevServer() {
  if (process.env.UI_BASE) return null;
  if (await listening(PORT)) return null;
  // Its own process group, so stopping it later stops Vite and esbuild too, not just the runner.
  const child = spawn("bun", ["run", "dev", "--", "--port", String(PORT)], { stdio: "ignore", detached: true });
  for (let i = 0; i < 60; i += 1) {
    if (await listening(PORT)) return child;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  process.kill(-child.pid);
  throw new Error(`the dev server did not come up on :${PORT}`);
}

async function runScene(browser, scene) {
  const context = await browser.newContext({ viewport: scene.viewport ?? VIEWPORT, deviceScaleFactor: 2 });
  await context.addInitScript((theme) => {
    localStorage.setItem("on-n-off.theme", theme);
    localStorage.setItem("on-n-off.screen", "overview");
  }, scene.theme ?? "dark");
  const page = await context.newPage();
  const problems = [];
  page.on("pageerror", (error) => problems.push(`pageerror: ${error.message}\n${error.stack ?? ""}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console.error: ${message.text()}`);
  });
  await page.goto(`${BASE}${scene.url}`, { waitUntil: "networkidle" });
  if (problems.some((problem) => problem.includes("Outdated Optimize Dep"))) {
    // Vite re-optimised its dependencies under us (a lockfile change); one reload settles it.
    problems.length = 0;
    await page.reload({ waitUntil: "networkidle" });
  }
  await page.waitForTimeout(150);
  let shots = 0;
  const shot = async (name) => {
    const file = path.join(OUT, `${name}.png`);
    await page.screenshot({ path: file });
    shots += 1;
    console.log(`  shot ${file}`);
  };
  for (const step of scene.steps ?? []) {
    if (step.click) await page.locator(step.click).first().click();
    else if (step.fill) await page.locator(step.fill).first().fill(step.text ?? "");
    else if (step.press) await page.keyboard.press(step.press);
    else if (step.hover) await page.locator(step.hover).first().hover();
    else if (step.wait) {
      if (typeof step.wait === "string") await page.locator(step.wait).first().waitFor();
      else await page.waitForTimeout(step.wait.ms ?? 200);
    } else if (step.shot) await shot(step.shot);
    await page.waitForTimeout(120);
  }
  if (!shots) await shot(scene.name);
  await context.close();
  return problems;
}

async function main() {
  const sceneFile = process.argv[2];
  const scenes = sceneFile ? JSON.parse(await readFile(sceneFile, "utf8")) : SCENES;
  mkdirSync(OUT, { recursive: true });
  const dev = await ensureDevServer();
  const browser = await chromium.launch();
  let failed = false;
  try {
    for (const scene of scenes) {
      console.log(`scene ${scene.name}`);
      const problems = await runScene(browser, scene);
      for (const problem of problems) {
        console.log(`  ! ${problem}`);
        failed = true;
      }
    }
  } finally {
    await browser.close();
    if (dev) process.kill(-dev.pid);
  }
  process.exit(failed ? 1 : 0);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
