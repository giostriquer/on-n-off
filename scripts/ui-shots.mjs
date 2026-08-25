#!/usr/bin/env node
// Screenshot harness: drives the UI in headless Chromium against the Vite dev server with the
// Tauri IPC mocked (`?mock=<scenario>`, see ui/src/dev/mockIpc.ts) and writes PNGs.
//
//   bun run ui:shots                      # the built-in scenes
//   bun run ui:shots scenes.json          # your own scenes (same shape as SCENES below)
//   UI_SHOTS_DIR=out bun run ui:shots     # output folder (default .tmp/ui-shots)
//
// A scene: { name, url, theme?: "dark"|"light", clock?: ISO instant, viewport?: {width,height},
// steps?: Step[] }. Steps run in order: {click: selector} · {fill: selector, text} · {press: key}
// · {hover: selector} · {wait: selector | {ms}} · {shot: name}. Selectors are Playwright selectors
// ("role=button[name=…]", "text=…", CSS) and must match exactly one element. Every scene ends with
// a screenshot named after the scene unless steps took one. The page clock is frozen (default: the
// fixtures' instant) so relative ages are reproducible; a scene fails on any page or console error.
// Starts its own Vite on UI_PORT (default 1425) so a running `tauri dev` on :1420 is left alone;
// set UI_BASE to point at an existing server instead.

import { spawn } from "node:child_process";
import { mkdirSync, openSync } from "node:fs";
import { readFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright";

const PORT = Number(process.env.UI_PORT ?? 1425);
const BASE = process.env.UI_BASE ?? `http://localhost:${PORT}`;
const OUT = process.env.UI_SHOTS_DIR ?? ".tmp/ui-shots";
const VIEWPORT = { width: 1120, height: 760 };
// Matches NOW in ui/src/dev/githubFixtures.ts, so "updated just now" / "5m ago" hold in captures.
const FIXTURE_CLOCK = "2026-08-24T20:00:00Z";
// localStorage keys the app reads at boot (ui/src/lib/theme.ts, features/session/SessionProvider.tsx).
// The saved screen is forced to "overview" because AppShell navigates to it on first route.
const THEME_KEY = "on-n-off.theme";
const SCREEN_KEY = "on-n-off.screen";
const STEP_KEYS = ["click", "fill", "press", "hover", "wait", "shot"];

const SCENES = [
  { name: "github-ok", url: "/github?mock=ok" },
  { name: "github-ok-light", url: "/github?mock=ok", theme: "light" },
  { name: "github-stale", url: "/github?mock=stale" },
  { name: "github-gh-missing", url: "/github?mock=ghMissing" },
  { name: "github-many", url: "/github?mock=many" },
  // Scrolled into the review list so an owner band is stuck under its section header.
  { name: "github-scrolled", url: "/github?mock=ok", steps: [{ click: "role=searchbox[name='Search pull requests']" }, { press: "PageDown" }, { press: "PageDown" }] },
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

function stopDevServer(child) {
  if (!child) return;
  // On POSIX the child leads its own process group, so this stops Vite and esbuild too.
  if (process.platform === "win32") child.kill();
  else process.kill(-child.pid);
}

async function ensureDevServer() {
  if (process.env.UI_BASE) return null;
  if (await listening(PORT)) return null;
  const log = path.join(OUT, "vite.log");
  const out = openSync(log, "w");
  const child = spawn("bun", ["run", "dev", "--", "--port", String(PORT)], {
    stdio: ["ignore", out, out],
    detached: process.platform !== "win32",
  });
  for (let i = 0; i < 60; i += 1) {
    if (await listening(PORT)) return child;
    if (child.exitCode !== null) break;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  stopDevServer(child);
  throw new Error(`the dev server did not come up on :${PORT}; see ${log}`);
}

function validateScene(scene, index) {
  if (!scene || typeof scene.name !== "string" || typeof scene.url !== "string") {
    throw new Error(`scene ${index}: needs a string "name" and "url"`);
  }
  for (const [stepIndex, step] of (scene.steps ?? []).entries()) {
    const keys = Object.keys(step).filter((key) => STEP_KEYS.includes(key));
    if (keys.length !== 1) {
      throw new Error(
        `scene ${scene.name} step ${stepIndex}: expected exactly one of ${STEP_KEYS.join("/")}, got ${JSON.stringify(step)}`,
      );
    }
  }
}

async function runScene(browser, scene) {
  const context = await browser.newContext({ viewport: scene.viewport ?? VIEWPORT, deviceScaleFactor: 2 });
  await context.addInitScript(
    ({ theme, themeKey, screenKey }) => {
      localStorage.setItem(themeKey, theme);
      localStorage.setItem(screenKey, "overview");
    },
    { theme: scene.theme ?? "dark", themeKey: THEME_KEY, screenKey: SCREEN_KEY },
  );
  const page = await context.newPage();
  await page.clock.setFixedTime(new Date(scene.clock ?? FIXTURE_CLOCK));
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
  // Screens mark themselves busy while their first read is in flight; wait for that, not a timer.
  await page.locator('[aria-busy="true"]').waitFor({ state: "detached", timeout: 10_000 }).catch(() => undefined);
  await page.waitForTimeout(100);
  let shots = 0;
  const shot = async (name) => {
    const file = path.join(OUT, `${name}.png`);
    await page.screenshot({ path: file });
    shots += 1;
    console.log(`  shot ${file}`);
  };
  for (const step of scene.steps ?? []) {
    // Locators are strict: a selector matching two elements fails the scene instead of guessing.
    if (step.click) await page.locator(step.click).click();
    else if (step.fill) await page.locator(step.fill).fill(step.text ?? "");
    else if (step.press) await page.keyboard.press(step.press);
    else if (step.hover) await page.locator(step.hover).hover();
    else if (step.wait) {
      if (typeof step.wait === "string") await page.locator(step.wait).waitFor();
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
  if (!Array.isArray(scenes)) throw new Error("the scene file must hold an array of scenes");
  scenes.forEach(validateScene);
  mkdirSync(OUT, { recursive: true });
  const dev = await ensureDevServer();
  const browser = await chromium.launch();
  let failed = false;
  try {
    for (const scene of scenes) {
      console.log(`scene ${scene.name}`);
      // A failing scene is reported and the run goes on, so one bad selector does not hide the rest.
      const problems = await runScene(browser, scene).catch((error) => [`scene failed: ${error.message}`]);
      for (const problem of problems) {
        console.log(`  ! ${problem}`);
        failed = true;
      }
    }
  } finally {
    await browser.close();
    stopDevServer(dev);
  }
  process.exit(failed ? 1 : 0);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
