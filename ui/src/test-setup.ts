import { cleanup } from "@testing-library/react";
import { afterAll, afterEach, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import "./tokens.css";

// Node ≥ 22 ships its own `localStorage` global (Web Storage behind `--localstorage-file`); when
// that flag is absent the object has no `getItem`, and Vitest's jsdom environment leaves it in
// place, so `localStorage.getItem is not a function` under Node 25. Give tests a real in-memory
// Storage in that case; environments that already provide a working one are untouched.
function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key: string) => values.get(key) ?? null,
    key: (index: number) => [...values.keys()][index] ?? null,
    removeItem: (key: string) => {
      values.delete(key);
    },
    setItem: (key: string, value: string) => {
      values.set(key, String(value));
    },
  };
}

for (const name of ["localStorage", "sessionStorage"] as const) {
  let usable = false;
  try {
    usable = typeof (globalThis as Record<string, unknown>)[name] === "object" &&
      typeof (globalThis as unknown as Record<string, Storage>)[name]?.getItem === "function";
  } catch {
    usable = false;
  }
  if (!usable) {
    Object.defineProperty(globalThis, name, { configurable: true, enumerable: true, value: memoryStorage() });
  }
}

afterEach(cleanup);

// Test files that share a vmForks worker share its `process`, so an env stub left by a file's last
// test would reach the next file in that worker; `unstubEnvs` cannot stop that, because it restores
// only before the next test of the same file. Undo them when each file ends.
afterAll(() => {
  vi.unstubAllEnvs();
});
