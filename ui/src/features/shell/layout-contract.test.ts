// @ts-expect-error Node builtins — vitest runs this file in Node.
import { readdirSync, readFileSync, statSync } from "node:fs";
// @ts-expect-error Node builtins — vitest runs this file in Node.
import { dirname, extname, join, relative } from "node:path";
// @ts-expect-error Node builtins — vitest runs this file in Node.
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = join(here, "..", "..");
const tokensPath = join(srcRoot, "tokens.css");
const shellPath = join(here, "AppShell.tsx");
const railPath = join(here, "LeftRail.tsx");

const PAGE_SCROLL = [
  /(?:^|[\s"'`])overflow-y-auto(?:[\s"'`]|$)/,
  /(?:^|[\s"'`])overflow-y-scroll(?:[\s"'`]|$)/,
  /(?:^|[\s"'`])overflow-auto(?:[\s"'`]|$)/,
  /overflow-y:\s*(?:auto|scroll)/,
  /overflow:\s*(?:auto|scroll)/,
];

const CUSTOM_SCROLL_BEHAVIOR = [
  /::-webkit-scrollbar/,
  /scrollbar-(?:color|width)\s*:/,
  /\bscrollbar(?:Color|Width)\s*:/,
  /\bscrollbar-(?:hidden|none|thin|thumb-[^\s"'`]+|track-[^\s"'`]+)\b/,
  /scroll-behavior\s*:/,
  /\bscrollBehavior\s*:/,
  /\bscroll-smooth\b/,
  /overscroll-behavior(?:-[xy])?\s*:/,
  /\boverscrollBehavior(?:X|Y)?\s*:/,
  /\boverscroll(?:-[xy])?-(?:contain|none)\b/,
];

const ALLOWED_SCROLLERS = new Set([
  relative(srcRoot, join(srcRoot, "tokens.css")).replaceAll("\\", "/"),
  relative(srcRoot, join(srcRoot, "features/scope/ScopeBar.css")).replaceAll("\\", "/"),
]);

function read(path: string) {
  return readFileSync(path, "utf8");
}

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      out.push(...walk(path));
      continue;
    }
    if ([".ts", ".tsx", ".css"].includes(extname(path))) {
      out.push(path);
    }
  }
  return out;
}

describe("shell layout contract", () => {
  it("recognizes CSS, Tailwind, and React scroll overrides", () => {
    const overrides = [
      ".panel { overscroll-behavior: contain; }",
      'className="overscroll-none scroll-smooth scrollbar-thin"',
      'style={{ overscrollBehavior: "contain", scrollBehavior: "smooth" }}',
      'style={{ scrollbarColor: "red blue", scrollbarWidth: "thin" }}',
    ];
    expect(
      overrides.filter((source) => CUSTOM_SCROLL_BEHAVIOR.some((pattern) => pattern.test(source))),
    ).toEqual(overrides);
  });

  it("locks the viewport so html/body/#root cannot scroll", () => {
    const css = read(tokensPath);
    const viewport = css.match(/html,\s*body,\s*#root\s*\{[^}]+\}/);
    expect(viewport, "tokens.css must style html, body, #root together").toBeTruthy();
    expect(viewport![0]).toMatch(/height:\s*100%/);
    expect(viewport![0]).toMatch(/overflow:\s*hidden/);
  });

  it("declares one named page scroller and a non-scrolling frame", () => {
    const css = read(tokensPath);
    expect(css).toMatch(/\.app-frame\s*\{[^}]*overflow:\s*hidden/);
    expect(css).toMatch(/\.app-body\s*\{[^}]*overflow:\s*hidden/);
    expect(css).toMatch(/\.app-body\s*\{[^}]*min-height:\s*0/);
    expect(css).toMatch(/\.app-scroll\s*\{[^}]*overflow-y:\s*auto/);
    expect(css).toMatch(/\.app-scroll\s*\{[^}]*min-height:\s*0/);
    expect(css).toMatch(/\.app-rail\s*\{[^}]*overflow:\s*hidden/);
    expect(css).toMatch(/\.app-rail\s*\{[^}]*min-height:\s*0/);
  });

  it("wires AppShell and LeftRail to those classes instead of utility overflow", () => {
    const shell = read(shellPath);
    const rail = read(railPath);
    expect(shell).toContain("app-frame");
    expect(shell).toContain("app-body");
    expect(shell).toContain('id="agent-panel"');
    expect(shell).toContain("app-scroll");
    expect(shell).not.toMatch(/overflow-y-auto/);
    expect(shell).not.toMatch(/min-h-full/);
    expect(rail).toContain("app-rail");
  });

  it("keeps overflow-y auto off route pages (only .app-scroll and .scope-list)", () => {
    const hits: string[] = [];
    for (const file of walk(srcRoot)) {
      const rel = relative(srcRoot, file).replaceAll("\\", "/");
      if (rel.endsWith(".test.ts") || rel.endsWith(".test.tsx")) {
        continue;
      }
      if (ALLOWED_SCROLLERS.has(rel)) {
        continue;
      }
      const text = read(file);
      for (const pattern of PAGE_SCROLL) {
        if (pattern.test(text)) {
          hits.push(rel);
          break;
        }
      }
    }
    expect(
      hits,
      `extra page scrollers recreate the double scrollbar: ${hits.join(", ")}. Put content in .app-scroll.`,
    ).toEqual([]);
  });

  it("leaves scrollbar appearance and scrolling behavior to the platform", () => {
    const hits: string[] = [];
    for (const file of walk(srcRoot)) {
      const rel = relative(srcRoot, file).replaceAll("\\", "/");
      if (rel.endsWith(".test.ts") || rel.endsWith(".test.tsx")) {
        continue;
      }
      const text = read(file);
      for (const pattern of CUSTOM_SCROLL_BEHAVIOR) {
        if (pattern.test(text)) {
          hits.push(rel);
          break;
        }
      }
    }
    expect(
      hits,
      `custom scroll behavior prevents native platform scrolling: ${hits.join(", ")}`,
    ).toEqual([]);
  });
});
