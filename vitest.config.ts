import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

export default defineConfig({
  plugins: [tailwindcss(), react()],
  resolve: {
    alias: {
      $lib: path.resolve(__dirname, "ui/src/lib"),
      "@": path.resolve(__dirname, "ui/src"),
    },
    conditions: ["browser"],
  },
  test: {
    environment: "jsdom",
    include: ["ui/src/**/*.test.ts", "ui/src/**/*.test.tsx"],
    setupFiles: ["./ui/src/test-setup.ts"],
    // A fresh VM context per test file inside reused worker processes: file isolation without
    // starting a process per file. Files in one worker share its `process`, so a `process.env`
    // write outlives its file (test-setup.ts undoes `vi.stubEnv`). They also share Node's
    // built-ins, whose errors fail `instanceof Error` in a test. Not vmThreads: it has segfaulted
    // here, and it ignores `env.TZ`.
    pool: "vmForks",
    // Fixtures fake fixed instants while the code reads the host's zone; pinned so a date means
    // the same day on every machine. See ui/src/test-environment.test.ts.
    env: { TZ: "UTC" },
  },
});
