import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

export default defineConfig({
  plugins: [tailwindcss(), svelte({ hot: false })],
  resolve: {
    alias: {
      $lib: path.resolve(__dirname, "ui/src/lib"),
    },
    conditions: ["browser"],
  },
  test: {
    environment: "jsdom",
    include: ["ui/src/**/*.test.ts"],
    setupFiles: ["./ui/src/test-setup.ts"],
  },
});
