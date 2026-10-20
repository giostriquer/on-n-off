import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { npmPackages, renderLicenses } from "./third-party-licenses.mjs";

function fixture(t, files) {
  const root = mkdtempSync(join(tmpdir(), "third-party-licenses-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [path, contents] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), typeof contents === "string" ? contents : JSON.stringify(contents));
  }
  return root;
}

const pkg = (name, version, license, dependencies = {}) => ({ name, version, license, dependencies });

test("the production npm tree is every runtime dependency, resolved the way Node resolves it", (t) => {
  const root = fixture(t, {
    "package.json": { dependencies: { app: "1", "@types/react": "1" }, devDependencies: { vite: "1" } },
    "node_modules/app/package.json": pkg("app", "1.0.0", "MIT", { shared: "2", helper: "1" }),
    "node_modules/app/LICENSE": "app license",
    "node_modules/app/node_modules/shared/package.json": pkg("shared", "2.0.0", "ISC"),
    "node_modules/app/node_modules/shared/LICENSE.md": "shared 2 license",
    "node_modules/shared/package.json": pkg("shared", "1.0.0", "ISC"),
    "node_modules/shared/LICENSE.md": "shared 1 license",
    "node_modules/helper/package.json": pkg("helper", "1.0.0", "MIT", { app: "1" }),
    "node_modules/helper/LICENSE.txt": "helper license",
    "node_modules/helper/NOTICE": "helper notice",
    "node_modules/@types/react/package.json": pkg("@types/react", "19.0.0", "MIT"),
    "node_modules/@types/react/LICENSE": "types license",
    "node_modules/vite/package.json": pkg("vite", "8.0.0", "MIT"),
    "node_modules/vite/LICENSE.md": "vite license",
  });
  assert.deepEqual(npmPackages(root), [
    { name: "app", version: "1.0.0", license: "MIT", texts: ["app license"] },
    { name: "helper", version: "1.0.0", license: "MIT", texts: ["helper license", "helper notice"] },
    { name: "shared", version: "2.0.0", license: "ISC", texts: ["shared 2 license"] },
  ]);
});

test("a package that ships no license text borrows the texts of the package it is mapped to, and an unmapped one fails", (t) => {
  const files = {
    "package.json": { dependencies: { "@tauri-apps/api": "2", "@tauri-apps/plugin-dialog": "2" } },
    "node_modules/@tauri-apps/api/package.json": pkg("@tauri-apps/api", "2.0.0", "Apache-2.0 OR MIT"),
    "node_modules/@tauri-apps/api/LICENSE_APACHE-2.0": "apache text",
    "node_modules/@tauri-apps/api/LICENSE_MIT": "mit text",
    "node_modules/@tauri-apps/plugin-dialog/package.json": pkg("@tauri-apps/plugin-dialog", "2.1.0", "MIT OR Apache-2.0"),
    "node_modules/@tauri-apps/plugin-dialog/LICENSE.spdx": "SPDXVersion: SPDX-2.1",
  };
  const borrowed = npmPackages(fixture(t, files)).find((entry) => entry.name === "@tauri-apps/plugin-dialog");
  assert.deepEqual(borrowed.texts, ["apache text", "mit text"]);
  const unmapped = fixture(t, {
    "package.json": { dependencies: { lonely: "1" } },
    "node_modules/lonely/package.json": pkg("lonely", "1.0.0", "MIT"),
  });
  assert.throws(() => npmPackages(unmapped), /lonely 1\.0\.0 ships no license text/);
});

test("a dependency that is not installed fails instead of going unlisted", (t) => {
  const root = fixture(t, {
    "package.json": { dependencies: { app: "1" } },
    "node_modules/app/package.json": pkg("app", "1.0.0", "MIT", { missing: "1" }),
    "node_modules/app/LICENSE": "app license",
  });
  assert.throws(() => npmPackages(root), /missing, a dependency of app 1\.0\.0, is not installed/);
});

test("the notice groups identical texts, lists their users, and leaves out the app itself", () => {
  const about = {
    licenses: [
      { id: "MIT", name: "MIT License", text: "mit text A", used_by: [{ crate: { name: "zeta", version: "1.0.0" } }, { crate: { name: "alpha", version: "0.2.0" } }] },
      { id: "MIT", name: "MIT License", text: "mit text own", used_by: [{ crate: { name: "on-n-off", version: "0.35.6" } }] },
      { id: "Apache-2.0", name: "Apache License 2.0", text: "apache text", used_by: [{ crate: { name: "beta", version: "3.0.0" } }] },
    ],
  };
  const npm = [
    { name: "react", version: "19.3.0", license: "MIT", texts: ["react text"] },
    { name: "react-dom", version: "19.3.0", license: "MIT", texts: ["react text"] },
  ];
  const notice = renderLicenses(about, npm, "on-n-off");
  assert.ok(!notice.includes("on-n-off 0.35.6"));
  assert.ok(!notice.includes("mit text own"));
  assert.match(notice, /Used by: alpha 0\.2\.0, zeta 1\.0\.0\n/);
  assert.match(notice, /Used by: react 19\.3\.0, react-dom 19\.3\.0\n/);
  assert.equal(notice.match(/react text/g).length, 1);
  assert.ok(notice.indexOf("## Rust crates") < notice.indexOf("apache text"));
  assert.ok(notice.indexOf("apache text") < notice.indexOf("## JavaScript packages"));
  assert.equal(renderLicenses(about, [...npm].reverse(), "on-n-off"), notice, "the output does not depend on input order");
});
