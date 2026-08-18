import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const INITIAL_JS_LIMIT_BYTES = 400_000;
const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const distDirectory = path.join(repositoryRoot, "ui", "dist");
const manifestPath = path.join(distDirectory, ".vite", "manifest.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
const entries = Object.entries(manifest).filter(([, chunk]) => chunk.isEntry);

if (entries.length !== 1) {
  throw new Error(`Expected one Vite entry chunk, found ${entries.length}`);
}

const [entryKey, entryChunk] = entries[0];
const initialChunkKeys = collectStaticImports(entryKey);
const initialJsFiles = [...initialChunkKeys]
  .map((key) => manifest[key]?.file)
  .filter((file) => typeof file === "string" && file.endsWith(".js"));
const initialJsSizes = await Promise.all(
  initialJsFiles.map(async (file) => ({
    file,
    bytes: (await stat(path.join(distDirectory, file))).size,
  })),
);
const initialJsBytes = initialJsSizes.reduce((total, item) => total + item.bytes, 0);

if (initialJsBytes > INITIAL_JS_LIMIT_BYTES) {
  throw new Error(
    `Initial JavaScript is ${initialJsBytes} bytes, above the ${INITIAL_JS_LIMIT_BYTES}-byte budget`,
  );
}

const requiredDynamicEntries = [
  "src/features/usage/UsageChart.tsx",
  "src/features/catalog/InstallSheet.tsx",
  "src/routes/overview.tsx",
  "src/routes/plugins.tsx",
  "src/routes/skills.tsx",
  "src/routes/mcp.tsx",
  "src/routes/usage.tsx",
  "src/routes/config.tsx",
  "src/routes/settings.tsx",
  "../node_modules/@tauri-apps/api/app.js",
  "../node_modules/@tauri-apps/plugin-updater/dist-js/index.js",
  "../node_modules/@tauri-apps/plugin-process/dist-js/index.js",
];

for (const key of requiredDynamicEntries) {
  const chunk = manifest[key];
  if (!chunk?.isDynamicEntry) {
    throw new Error(`Expected ${key} to be a dynamic entry`);
  }
  if (!entryChunk.dynamicImports?.includes(key)) {
    throw new Error(`Expected the entry manifest to reference lazy chunk ${key}`);
  }
  if (initialChunkKeys.has(key)) {
    throw new Error(`Lazy chunk ${key} is also part of the initial static graph`);
  }
}

console.log(
  `Initial JavaScript: ${initialJsBytes} bytes (${(initialJsBytes / 1_000).toFixed(2)} kB) / ${(INITIAL_JS_LIMIT_BYTES / 1_000).toFixed(0)} kB`,
);
for (const item of initialJsSizes) {
  console.log(`  ${item.file}: ${item.bytes} bytes`);
}
console.log(`Verified ${requiredDynamicEntries.length} lazy route, chart, and updater entries.`);

function collectStaticImports(startKey) {
  const visited = new Set();

  function visit(key) {
    if (visited.has(key)) {
      return;
    }
    const chunk = manifest[key];
    if (!chunk) {
      throw new Error(`Manifest references missing chunk ${key}`);
    }
    visited.add(key);
    for (const importedKey of chunk.imports ?? []) {
      visit(importedKey);
    }
  }

  visit(startKey);
  return visited;
}
