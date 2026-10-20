import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const LICENSE_FILE = /^(third[-_]party[-_])?(licen[cs]e|copying|notice)/i;
const BUILD_OUTPUT = ["rolldown", "tailwindcss", "vite"];
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
  return readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && LICENSE_FILE.test(entry.name) && !/\.spdx$/i.test(entry.name))
    .map((entry) => entry.name)
    .sort(byText)
    .map((file) => readFileSync(join(dir, file), "utf8").trim());
}

function describe(dir) {
  const manifest = readJson(join(dir, "package.json"));
  if (!manifest.license) throw new Error(`${label(manifest)} declares no license`);
  return { manifest, entry: { name: manifest.name, version: manifest.version, license: manifest.license, texts: licenseTexts(dir) } };
}

export function npmPackages(root, buildOutput = BUILD_OUTPUT) {
  const found = new Map();
  const add = (dir) => {
    const { manifest, entry } = describe(dir);
    const key = `${entry.name}@${entry.version}`;
    if (found.has(key)) return null;
    found.set(key, entry);
    return manifest;
  };
  const visit = (dir) => {
    const manifest = add(dir);
    if (manifest) walk(manifest, dir, label(manifest));
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
  for (const name of buildOutput) {
    const resolved = resolvePackage(root, name, root);
    if (!resolved) throw new Error(`${name}, which the build emits into the app, is not installed`);
    add(resolved);
  }
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

export function renderLicenses(about, npm) {
  const crates = about.licenses.flatMap((license) =>
    license.used_by.map(({ crate }) => ({ heading: `${license.name} (${license.id})`, text: license.text.trim(), user: label(crate) })),
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
  execFileSync("cargo", ["fetch", "--locked", "--manifest-path", manifest], { stdio: "inherit" });
  const about = execFileSync(process.env.CARGO_ABOUT ?? "cargo-about", ["generate", "--format", "json", "--locked", "--offline", "--fail", "--manifest-path", manifest], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    maxBuffer: 1 << 30,
  });
  const notice = renderLicenses(JSON.parse(about), npmPackages(root));
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, notice);
  console.log(`wrote ${output}`);
}
