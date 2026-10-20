import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { npmPackages, renderLicenses } from "./third-party-licenses.mjs";

const here = dirname(fileURLToPath(import.meta.url));

function fixture(t, files) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "third-party-licenses-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [path, contents] of Object.entries(files).reverse()) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), typeof contents === "string" ? contents : JSON.stringify(contents));
  }
  return root;
}

const pkg = (name, version, license, dependencies = {}, extra = {}) => ({ name, version, license, dependencies, ...extra });

test("the production npm tree is every runtime dependency, resolved the way Node resolves it", (t) => {
  const root = fixture(t, {
    "package.json": { dependencies: { app: "1", "@types/react": "1" }, devDependencies: { vite: "1" } },
    "node_modules/app/package.json": pkg("app", "1.0.0", "MIT", { shared: "2", helper: "1" }, { optionalDependencies: { present: "1", absent: "1" } }),
    "node_modules/app/LICENSE": "app license",
    "node_modules/app/node_modules/shared/package.json": pkg("shared", "2.0.0", "ISC"),
    "node_modules/app/node_modules/shared/LICENSE.md": "shared 2 license\n",
    "node_modules/shared/package.json": pkg("shared", "1.0.0", "ISC"),
    "node_modules/shared/LICENSE.md": "shared 1 license",
    "node_modules/helper/package.json": pkg("helper", "1.0.0", "MIT", { app: "1" }),
    "node_modules/helper/LICENSE.txt": "helper license",
    "node_modules/helper/NOTICE": "helper notice",
    "node_modules/helper/LICENSES/MIT.txt": "a folder of licenses, not a license file",
    "node_modules/present/package.json": pkg("present", "1.0.0", "MIT"),
    "node_modules/present/LICENSE": "present license",
    "node_modules/@types/react/package.json": pkg("@types/react", "19.0.0", "MIT"),
    "node_modules/@types/react/LICENSE": "types license",
    "node_modules/vite/package.json": pkg("vite", "8.0.0", "MIT"),
    "node_modules/vite/LICENSE.md": "vite license",
  });
  assert.deepEqual(npmPackages(root, []), [
    { name: "app", version: "1.0.0", license: "MIT", texts: ["app license"] },
    { name: "helper", version: "1.0.0", license: "MIT", texts: ["helper license", "helper notice"] },
    { name: "present", version: "1.0.0", license: "MIT", texts: ["present license"] },
    { name: "shared", version: "2.0.0", license: "ISC", texts: ["shared 2 license"] },
  ]);
});

test("the build tools whose code lands in the app are listed too, without their own dependencies", (t) => {
  const root = fixture(t, {
    "package.json": { dependencies: {}, devDependencies: { vite: "1" } },
    "node_modules/vite/package.json": pkg("vite", "8.0.0", "MIT", { rolldown: "1" }),
    "node_modules/vite/LICENSE.md": "vite license",
    "node_modules/rolldown/package.json": pkg("rolldown", "1.0.0", "MIT"),
    "node_modules/rolldown/LICENSE": "rolldown license",
    "node_modules/rolldown/THIRD-PARTY-LICENSE": "esbuild license",
  });
  assert.deepEqual(npmPackages(root, ["vite"]), [{ name: "vite", version: "8.0.0", license: "MIT", texts: ["vite license"] }]);
  assert.deepEqual(npmPackages(root, ["rolldown"])[0].texts, ["rolldown license", "esbuild license"], "vendored code's licenses come along");
  assert.throws(() => npmPackages(root, ["tailwindcss"]), /tailwindcss, which the build emits into the app, is not installed/);
});

test("a package that ships no license text borrows the texts of the package it is mapped to, and an unmapped one fails", (t) => {
  const mapped = fixture(t, {
    "package.json": { dependencies: { "@tanstack/charts": "1", "@tauri-apps/plugin-dialog": "2", "@tauri-apps/api": "2" } },
    "node_modules/@tanstack/charts/package.json": pkg("@tanstack/charts", "1.0.0", "MIT"),
    "node_modules/@tanstack/charts/LICENSE": "charts text",
    "node_modules/@tauri-apps/api/package.json": pkg("@tauri-apps/api", "2.0.0", "Apache-2.0 OR MIT"),
    "node_modules/@tauri-apps/api/LICENSE_APACHE-2.0": "apache text",
    "node_modules/@tauri-apps/api/LICENSE_MIT": "mit text",
    "node_modules/@tauri-apps/plugin-dialog/package.json": pkg("@tauri-apps/plugin-dialog", "2.1.0", "MIT OR Apache-2.0"),
    "node_modules/@tauri-apps/plugin-dialog/LICENSE.spdx": "SPDXVersion: SPDX-2.1",
  });
  const borrowed = npmPackages(mapped, []).find((entry) => entry.name === "@tauri-apps/plugin-dialog");
  assert.deepEqual(borrowed.texts, ["apache text", "mit text"]);
  const unmapped = fixture(t, {
    "package.json": { dependencies: { other: "1", lonely: "1" } },
    "node_modules/other/package.json": pkg("other", "1.0.0", "MIT"),
    "node_modules/other/LICENSE": "other text",
    "node_modules/lonely/package.json": pkg("lonely", "1.0.0", "MIT"),
  });
  assert.throws(() => npmPackages(unmapped, []), /lonely 1\.0\.0 ships no license text/);
});

test("a dependency that is not installed, or a package that declares no license, fails instead of going unlisted", (t) => {
  const missing = fixture(t, {
    "package.json": { dependencies: { app: "1" } },
    "node_modules/app/package.json": pkg("app", "1.0.0", "MIT", { missing: "1" }),
    "node_modules/app/LICENSE": "app license",
  });
  assert.throws(() => npmPackages(missing, []), /missing, a dependency of app 1\.0\.0, is not installed/);
  const undeclared = fixture(t, {
    "package.json": { dependencies: { quiet: "1" } },
    "node_modules/quiet/package.json": { name: "quiet", version: "1.0.0" },
    "node_modules/quiet/LICENSE": "quiet text",
  });
  assert.throws(() => npmPackages(undeclared, []), /quiet 1\.0\.0 declares no license/);
});

const about = {
  licenses: [
    { id: "MIT", name: "MIT License", text: "mit text B\n", used_by: [{ crate: { name: "gamma", version: "2.0.0" } }] },
    { id: "MIT", name: "MIT License", text: "mit text A", used_by: [{ crate: { name: "zeta", version: "1.0.0" } }, { crate: { name: "alpha", version: "0.2.0" } }] },
    { id: "Apache-2.0", name: "Apache License 2.0", text: "apache text", used_by: [{ crate: { name: "beta", version: "3.0.0" } }] },
  ],
};
const npm = [
  { name: "react-dom", version: "19.3.0", license: "MIT", texts: ["react text"] },
  { name: "@tauri-apps/api", version: "2.0.0", license: "MIT OR Apache-2.0", texts: ["apache text", "mit text"] },
  { name: "react", version: "19.3.0", license: "MIT", texts: ["react text"] },
];
const expectedNotice = `# Third-party licenses

on-n-off includes the open-source software below. Each license text appears once, followed by the packages it covers.

## Rust crates

### Apache License 2.0 (Apache-2.0)

Used by: beta 3.0.0

~~~~text
apache text
~~~~

### MIT License (MIT)

Used by: alpha 0.2.0, zeta 1.0.0

~~~~text
mit text A
~~~~

### MIT License (MIT)

Used by: gamma 2.0.0

~~~~text
mit text B
~~~~

## JavaScript packages

### MIT

Used by: react 19.3.0, react-dom 19.3.0

~~~~text
react text
~~~~

### MIT OR Apache-2.0

Used by: @tauri-apps/api 2.0.0

~~~~text
apache text

mit text
~~~~
`;

test("the notice groups identical texts under their license, sorted, each text fenced once", () => {
  assert.equal(renderLicenses(about, npm), expectedNotice);
});

test("the notice does not depend on the order cargo-about or the npm walk returns things in", () => {
  const shuffled = {
    licenses: [...about.licenses].reverse().map((license) => ({ ...license, used_by: [...license.used_by].reverse() })),
  };
  assert.equal(renderLicenses(shuffled, [...npm].reverse()), expectedNotice);
});

test("the command fetches the crates, runs cargo-about with its failure gates, and writes the notice", { skip: process.platform === "win32" ? "fake executables are shell scripts" : false }, (t) => {
  const root = fixture(t, {
    "package.json": { dependencies: { app: "1" } },
    "node_modules/app/package.json": pkg("app", "1.0.0", "MIT"),
    "node_modules/app/LICENSE": "app license",
    "node_modules/tailwindcss/package.json": pkg("tailwindcss", "4.0.0", "MIT"),
    "node_modules/tailwindcss/LICENSE": "tailwind license",
    "node_modules/vite/package.json": pkg("vite", "8.0.0", "MIT"),
    "node_modules/vite/LICENSE.md": "vite license",
    "node_modules/rolldown/package.json": pkg("rolldown", "1.0.0", "MIT"),
    "node_modules/rolldown/LICENSE": "rolldown license",
    "src-tauri/Cargo.toml": "[package]\nname = \"webapp\"\n",
    "about.json": about,
    "bin/cargo": `#!/bin/sh\necho "$@" >> "$(dirname "$0")/cargo.log"\n`,
    "bin/cargo-about": [
      "#!/bin/sh",
      'dir="$(dirname "$0")"',
      '[ -f "$dir/cargo.log" ] || exit 4',
      'echo "$@" > "$dir/cargo-about.log"',
      '[ -n "$FAIL" ] && exit 3',
      'out=""',
      'while [ $# -gt 0 ]; do [ "$1" = "--output-file" ] && out="$2"; shift; done',
      '[ -n "$out" ] || { echo "cargo-about should not redirect its output in powershell" >&2; exit 5; }',
      'cat "$dir/../about.json" > "$out"',
      "",
    ].join("\n"),
  });
  for (const tool of ["cargo", "cargo-about"]) chmodSync(join(root, "bin", tool), 0o755);
  mkdirSync(join(root, "scripts"));
  copyFileSync(join(here, "third-party-licenses.mjs"), join(root, "scripts", "third-party-licenses.mjs"));
  const manifest = join(root, "src-tauri", "Cargo.toml");
  const run = (output, env = {}) =>
    execFileSync(process.execPath, [join(root, "scripts", "third-party-licenses.mjs"), output], {
      env: { ...process.env, PATH: `${join(root, "bin")}:${process.env.PATH}`, CARGO_ABOUT: join(root, "bin", "cargo-about"), ...env },
      stdio: "pipe",
    });

  const output = join(root, "out", "THIRD_PARTY_LICENSES.md");
  run(output);
  assert.equal(readFileSync(join(root, "bin", "cargo.log"), "utf8"), `fetch --locked --manifest-path ${manifest}\n`);
  assert.match(
    readFileSync(join(root, "bin", "cargo-about.log"), "utf8"),
    new RegExp(`^generate --format json --locked --offline --fail --manifest-path ${manifest.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} --output-file \\S+\\n$`),
    "cargo-about writes its JSON to a file: it refuses stdout under PowerShell",
  );
  const notice = readFileSync(output, "utf8");
  assert.ok(notice.startsWith(expectedNotice.split("## JavaScript packages")[0]), "the crates come from cargo-about's answer");
  for (const user of ["app 1.0.0", "rolldown 1.0.0", "tailwindcss 4.0.0", "vite 8.0.0"]) assert.ok(notice.includes(user), user);

  const failed = join(root, "out", "failed.md");
  assert.throws(() => run(failed, { FAIL: "1" }));
  assert.equal(existsSync(failed), false, "a failed cargo-about writes no notice");
});
