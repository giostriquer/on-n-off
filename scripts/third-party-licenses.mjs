import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const LICENSE_FILE = /^(licen[cs]e|copying|notice)/i;
const TEXT_FROM = {
  "@tauri-apps/plugin-dialog": "@tauri-apps/api",
  "@tauri-apps/plugin-process": "@tauri-apps/api",
  "@tauri-apps/plugin-updater": "@tauri-apps/api",
};

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const label = ({ name, version }) => `${name} ${version}`;
const byText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

function resolvePackage(root, name, from) {
  for (let dir = from; !relative(root, dir).startsWith(".."); dir = dirname(dir)) {
    const candidate = join(dir, "node_modules", name);
    if (existsSync(join(candidate, "package.json"))) return candidate;
    if (dir === root) break;
  }
  return null;
}

function licenseTexts(dir) {
  return readdirSync(dir)
    .filter((file) => LICENSE_FILE.test(file) && !/\.spdx$/i.test(file))
    .sort(byText)
    .map((file) => readFileSync(join(dir, file), "utf8").trim());
}

export function npmPackages(root) {
  const found = new Map();
  const visit = (dir) => {
    const manifest = readJson(join(dir, "package.json"));
    const key = `${manifest.name}@${manifest.version}`;
    if (found.has(key)) return;
    found.set(key, { name: manifest.name, version: manifest.version, license: manifest.license, texts: licenseTexts(dir) });
    walk(manifest, dir, label(manifest));
  };
  const walk = (manifest, dir, owner) => {
    const optional = new Set(Object.keys(manifest.optionalDependencies ?? {}));
    for (const name of Object.keys({ ...manifest.dependencies, ...manifest.optionalDependencies })) {
      if (name.startsWith("@types/")) continue;
      const resolved = resolvePackage(root, name, dir);
      if (resolved) visit(resolved);
      else if (!optional.has(name)) throw new Error(`${name}, a dependency of ${owner}, is not installed`);
    }
  };
  walk(readJson(join(root, "package.json")), root, "the app");
  const packages = [...found.values()];
  for (const entry of packages.filter((candidate) => candidate.texts.length === 0)) {
    const source = packages.find((candidate) => candidate.name === TEXT_FROM[entry.name] && candidate.texts.length > 0);
    if (!source) throw new Error(`${label(entry)} ships no license text; map it in TEXT_FROM to a package that does`);
    entry.texts = source.texts;
  }
  return packages.sort((a, b) => byText(a.name, b.name) || byText(a.version, b.version));
}

function section(title, groups) {
  const lines = [`## ${title}`, ""];
  for (const group of [...groups].sort((a, b) => byText(a.heading, b.heading) || byText(a.users[0], b.users[0]))) {
    lines.push(`### ${group.heading}`, "", `Used by: ${group.users.join(", ")}`, "", "~~~~text", group.text, "~~~~", "");
  }
  return lines;
}

function grouped(entries) {
  const groups = new Map();
  for (const { heading, text, user } of entries) {
    const group = groups.get(text) ?? { heading, text, users: [] };
    group.users.push(user);
    groups.set(text, group);
  }
  for (const group of groups.values()) group.users.sort(byText);
  return groups.values();
}

export function renderLicenses(about, npm, appCrate) {
  const crates = about.licenses.flatMap((license) =>
    license.used_by
      .filter(({ crate }) => crate.name !== appCrate)
      .map(({ crate }) => ({ heading: `${license.name} (${license.id})`, text: license.text.trim(), user: label(crate) })),
  );
  const packages = npm.map((entry) => ({ heading: entry.license, text: entry.texts.join("\n\n"), user: label(entry) }));
  return [
    "# Third-party licenses",
    "",
    "on-n-off includes the open-source software below. Each license text appears once, followed by the packages it covers.",
    "",
    ...section("Rust crates", grouped(crates)),
    ...section("JavaScript packages", grouped(packages)),
  ].join("\n");
}

if (import.meta.main) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const manifest = join(root, "src-tauri", "Cargo.toml");
  const output = process.argv[2] ?? join(root, "src-tauri", "gen", "THIRD_PARTY_LICENSES.md");
  const scratch = mkdtempSync(join(tmpdir(), "on-n-off-licenses-"));
  try {
    execFileSync("cargo", ["fetch", "--locked", "--manifest-path", manifest], { stdio: "inherit" });
    const json = join(scratch, "about.json");
    execFileSync(process.env.CARGO_ABOUT ?? "cargo-about", ["generate", "--format", "json", "--locked", "--offline", "--fail", "--manifest-path", manifest, "--output-file", json], { stdio: "inherit" });
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, renderLicenses(readJson(json), npmPackages(root), "on-n-off"));
    console.log(`wrote ${output}`);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}
