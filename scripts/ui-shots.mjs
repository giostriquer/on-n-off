#!/usr/bin/env node

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
const FIXTURE_CLOCK = "2026-08-24T20:00:00Z";
const THEME_KEY = "on-n-off.theme";
const SCREEN_KEY = "on-n-off.screen";
const STEP_KEYS = ["click", "fill", "press", "hover", "scroll", "wait", "shot"];

const ARCHIVED = {
  claude: "role=region[name='Claude archived accounts']",
  codex: "role=region[name='Codex archived accounts']",
};
const ARCHIVED_OPEN = [
  { click: `${ARCHIVED.claude} >> role=button[name='Archived (2)']` },
  { click: `${ARCHIVED.codex} >> role=button[name='Archived (1)']` },
  { wait: "role=button[name='Unarchive former@example.com']" },
];
const ARCHIVE_ACTION = "role=group[name='Actions for person@acme.example'] >> role=button[name='Archive account']";

const SCENES = [
  { name: "github-ok", url: "/github?mock=ok" },
  { name: "github-ok-light", url: "/github?mock=ok", theme: "light" },
  { name: "github-stale", url: "/github?mock=stale" },
  { name: "github-gh-missing", url: "/github?mock=ghMissing" },
  { name: "github-many", url: "/github?mock=many" },
  { name: "github-scrolled", url: "/github?mock=ok", steps: [{ click: "role=searchbox[name='Search pull requests']" }, { press: "PageDown" }, { press: "PageDown" }] },
  { name: "overview-catalog", url: "/overview?mock=catalog", steps: [{ scroll: "text=Live on this scope" }] },
  { name: "overview-catalog-wide", url: "/overview?mock=catalog", viewport: { width: 1440, height: 900 }, steps: [{ scroll: "text=Live on this scope" }] },
  { name: "overview-catalog-light", url: "/overview?mock=catalog", theme: "light", steps: [{ scroll: "text=Live on this scope" }] },
  { name: "overview-empty", url: "/overview?mock=ok", steps: [{ scroll: "text=Live on this scope" }] },
  { name: "settings-github", url: "/settings?mock=ok", steps: [{ wait: "role=region[name='Pull requests']" }] },
  { name: "hooks", url: "/hooks?mock=hooks", steps: [
    { wait: "role=heading[name='Hooks']" },
    { shot: "hooks" },
    { click: "role=tab[name='Codex']" },
    { wait: "text=acme-webapp-tools" },
    { shot: "hooks-codex" },
  ] },
  { name: "limits-band", url: "/limits?mock=limitsBand", steps: [{ wait: "role=region[name='Codex limits · 50% of the week']" }] },
  { name: "limits-band-light", url: "/limits?mock=limitsBand", theme: "light", steps: [{ wait: "role=region[name='Codex limits · 50% of the week']" }] },
  { name: "limits-ok", url: "/limits?mock=ok", steps: [{ wait: "role=region[name='Codex limits · person@acme.example']" }] },
  { name: "limits-ok-light", url: "/limits?mock=ok", theme: "light", steps: [{ wait: "role=region[name='Codex limits · person@acme.example']" }] },
  { name: "limits-popover", url: "/?surface=limits-popover&mock=ok", viewport: { width: 350, height: 480 }, steps: [{ wait: "role=article[name='Codex limits · other@example.com']" }] },
  { name: "limits-archived", url: "/limits?mock=archivedAccounts", viewport: { width: 1120, height: 1400 }, steps: [{ wait: `${ARCHIVED.claude} >> role=button[name='Archived (2)']` }] },
  { name: "limits-archived-open", url: "/limits?mock=archivedAccounts", viewport: { width: 1120, height: 1400 }, steps: ARCHIVED_OPEN },
  { name: "limits-archived-open-light", url: "/limits?mock=archivedAccounts", theme: "light", viewport: { width: 1120, height: 1400 }, steps: ARCHIVED_OPEN },
  { name: "limits-archive-menu", url: "/limits?mock=archivedAccounts", viewport: { width: 1120, height: 1400 }, steps: [
    { click: "role=button[name='More actions for person@acme.example']" },
    { wait: ARCHIVE_ACTION },
    { shot: "limits-archive-menu" },
    { click: ARCHIVE_ACTION },
    { click: `${ARCHIVED.codex} >> role=button[name='Archived (2)']` },
    { wait: "role=button[name='Unarchive person@acme.example']" },
    { shot: "limits-archived-after-archive" },
  ] },
  { name: "popover-archived", url: "/?surface=limits-popover&mock=archivedAccounts", viewport: { width: 350, height: 900 }, steps: [{ wait: "role=article[name='Codex limits · other@example.com']" }] },
];

function connects(port, host) {
  return new Promise((resolve) => {
    const socket = net.connect({ port, host });
    socket.once("connect", () => { socket.end(); resolve(true); });
    socket.once("error", () => resolve(false));
  });
}

async function listening(port) {
  for (const host of ["127.0.0.1", "::1"]) {
    if (await connects(port, host)) return true;
  }
  return false;
}

function stopDevServer(child) {
  if (!child) return;
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
    problems.length = 0;
    await page.reload({ waitUntil: "networkidle" });
  }
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
    if (step.click) await page.locator(step.click).click();
    else if (step.fill) await page.locator(step.fill).fill(step.text ?? "");
    else if (step.press) await page.keyboard.press(step.press);
    else if (step.hover) await page.locator(step.hover).hover();
    else if (step.scroll) await scrollTo(page, step.scroll);
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

async function scrollTo(page, selector) {
  await page.locator(selector).evaluate((node) => {
    node.scrollIntoView({ block: "start" });
    for (let parent = node.parentElement; parent; parent = parent.parentElement) {
      const overflow = getComputedStyle(parent).overflowY;
      if (overflow === "auto" || overflow === "scroll") {
        parent.scrollTop -= 14;
        return;
      }
    }
  });
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
