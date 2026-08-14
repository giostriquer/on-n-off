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
  },
});
