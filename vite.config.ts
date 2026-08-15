import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  root: "ui",
  plugins: [tailwindcss(), react()],
  resolve: {
    alias: {
      $lib: path.resolve(__dirname, "ui/src/lib"),
      "@": path.resolve(__dirname, "ui/src"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    outDir: path.resolve(__dirname, "ui/dist"),
    emptyOutDir: true,
    manifest: true,
  },
});
