import { cleanup } from "@testing-library/react";
import { afterAll, afterEach, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import "./tokens.css";

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

afterAll(() => {
  vi.unstubAllEnvs();
});
