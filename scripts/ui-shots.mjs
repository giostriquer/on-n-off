#!/usr/bin/env node

import { spawn } from "node:child_process";
import { mkdirSync, openSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright";
import { chooseScenes } from "./ui-scenes.mjs";

const PORT = Number(process.env.UI_PORT ?? 1425);
const BASE = process.env.UI_BASE ?? `http://localhost:${PORT}`;
const OUT = process.env.UI_SHOTS_DIR ?? ".tmp/ui-shots";
const VIEWPORT = { width: 1120, height: 760 };
const FIXTURE_CLOCK = "2026-08-24T20:00:00Z";
const THEME_KEY = "on-n-off.theme";
const SCREEN_KEY = "on-n-off.screen";

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

function accountHeaderIssues(element) {
  const title = element.querySelector(".truncate");
  const bounds = element.getBoundingClientRect();
  const issues = [];
  if (title.getBoundingClientRect().width < Math.min(100, title.scrollWidth)) {
    issues.push(`account name has less than 100px to display: ${title.textContent}`);
  }
  for (const button of element.querySelectorAll("button")) {
    const rect = button.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) continue;
    if (rect.left < bounds.left - 1 || rect.right > bounds.right + 1) {
      issues.push(`account header action overflows: ${button.getAttribute("aria-label") ?? button.textContent}`);
    }
  }
  const tags = element.querySelector('[data-testid="account-tags"]');
  if (!tags) return { measured: false, issues };
  const name = title.getBoundingClientRect();
  const line = title.parentElement;
  const column = line.getBoundingClientRect();
  const contentRight = bounds.right - parseFloat(getComputedStyle(element).paddingRight);
  const tagRects = [...tags.children].map(child => child.getBoundingClientRect());
  const middle = rect => (rect.top + rect.bottom) / 2;
  const controls = [...element.querySelectorAll("button, [role='img']")]
    .filter(node => !tags.contains(node) && node.getBoundingClientRect().width > 0);
  for (const control of controls) {
    if (Math.abs(middle(control.getBoundingClientRect()) - middle(name)) > 2) {
      issues.push(`account header control leaves the name's line: ${control.getAttribute("aria-label") ?? control.tagName} on ${title.textContent}`);
    }
  }
  if (tagRects.length > 0) {
    const gap = node => parseFloat(getComputedStyle(node).columnGap) || 0;
    const range = document.createRange();
    range.selectNodeContents(title);
    const nameWidth = range.getBoundingClientRect().width;
    const tagsWidth = tagRects.reduce((sum, rect) => sum + rect.width, 0) + gap(tags) * (tagRects.length - 1);
    const fits = name.left + nameWidth + gap(line) + tagsWidth <= column.right - 1;
    const inline = tagRects.every(rect => rect.top < name.bottom && rect.bottom > name.top);
    if (fits && !inline) {
      issues.push(`account tags wrap below the name although they fit beside it: ${title.textContent}`);
    }
    if (inline && title.scrollWidth > title.clientWidth + 0.5) {
      issues.push(`account tags beside the name truncate it: ${title.textContent}`);
    }
    if (inline && Math.abs(Math.max(...tagRects.map(rect => rect.right)) - column.right) > 1) {
      issues.push(`account tags beside the name are not attached at the header's right: ${title.textContent}`);
    }
    if (!inline && Math.abs(Math.min(...tagRects.map(rect => rect.left)) - column.left) > 1) {
      issues.push(`account tags on their own row do not start under the name: ${title.textContent}`);
    }
    if (!inline && Math.max(...tagRects.map(rect => rect.right)) > contentRight + 1) {
      issues.push(`account tags on their own row reach past the header's right edge: ${title.textContent}`);
    }
    if (!inline && Math.min(...tagRects.map(rect => rect.top)) < name.bottom - 1) {
      issues.push(`account tags on their own row are not under the name: ${title.textContent}`);
    }
  }
  const category = element.querySelector('[data-testid="account-category"]');
  if (category) {
    const rect = category.getBoundingClientRect();
    const above = Math.max(name.bottom, ...tagRects.map(tag => tag.bottom));
    if (Math.abs(rect.left - column.left) > 1 || rect.top < above - 1) {
      issues.push(`account category is not under the name and its tags: ${title.textContent}`);
    }
    if (Math.abs(rect.right - contentRight) > 1) {
      issues.push(`account category does not run to the header's right edge: ${title.textContent}`);
    }
  }
  return { measured: true, issues };
}

function popoverHeaderIssues(element) {
  const row = element.querySelector(":scope > div");
  const label = row?.querySelector(".truncate");
  const tags = [...(row?.children ?? [])].filter(child => child !== label && child.getBoundingClientRect().width > 0);
  if (!label || tags.length === 0) return { measured: false, issues: [] };
  const right = Math.max(...tags.map(tag => tag.getBoundingClientRect().right));
  return {
    measured: true,
    issues: Math.abs(right - row.getBoundingClientRect().right) > 1
      ? [`popover account tags are not attached at the header's right: ${label.textContent}`]
      : [],
  };
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
  for (const tab of await page.getByRole("tab").all()) {
    const clipped = await tab.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      const group = element.closest('[role="tablist"]').getBoundingClientRect();
      return rect.left < Math.max(0, group.left) - 1 || rect.right > Math.min(innerWidth, group.right) + 1;
    });
    if (clipped) problems.push(`provider tab clipped: ${await tab.textContent()}`);
  }
  if (scene.limitsColumns) {
    const headers = page.locator('[data-testid="limits-screen"] header').filter({ has: page.locator(".truncate") });
    const columns = await headers.evaluateAll(elements => new Set(elements.map(element => Math.round(element.getBoundingClientRect().left))).size);
    if (columns !== scene.limitsColumns) problems.push(`expected ${scene.limitsColumns} provider columns, got ${columns}`);
    problems.push(...await headerProblems(headers, accountHeaderIssues, "the account tag guard measured no tag container"));
  }
  if (scene.popoverHeaders) {
    problems.push(...await headerProblems(page.locator("article > header"), popoverHeaderIssues, "the popover tag guard measured no tags"));
  }
  if (scene.checkMuseChart) {
    const blueLine = page.locator('.usage-chart-host path[stroke="#0082fb"]');
    if (await blueLine.count() === 0) problems.push("Muse chart line is not blue");
    await page.locator(".usage-chart-host svg").hover();
    const museRow = page.locator(".usage-chart-tooltip .ts-chart-tooltip__row")
      .filter({ has: page.getByText("Muse", { exact: true }) });
    await museRow.waitFor();
    const color = await museRow.locator(".ts-chart-tooltip__swatch")
      .evaluate(element => getComputedStyle(element).backgroundColor);
    if (color !== "rgb(0, 130, 251)") problems.push(`Muse chart series is not blue: ${color}`);
  }
  await context.close();
  return problems;
}

async function headerProblems(headers, check, nothingMeasured) {
  const problems = [];
  let measured = 0;
  for (const header of await headers.all()) {
    const result = await header.evaluate(check);
    if (result.measured) measured += 1;
    problems.push(...result.issues);
  }
  return measured === 0 ? [...problems, nothingMeasured] : problems;
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
  const scenes = await chooseScenes(process.argv.slice(2));
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
