#!/usr/bin/env node
// Verifies a GitHub release of on-n-off before (or after) it is published.
//
//   bun scripts/verify-release.mjs <vX.Y.Z> <previous vP.Q.R>
//       [--repo owner/name] [--dir .tmp/release-check] [--no-download] [--allow-asset-change]
//
// Downloads the release's assets and body with `gh` (drafts included), plus the previous release's
// asset names and latest.json, and checks:
//   1. the asset set is the previous release's, renamed to the new version;
//   2. SHA256SUMS.txt lists exactly the other assets, each with its own hash;
//   3. every `.sig` is a minisign signature (Ed25519 over BLAKE2b-512) for the file it names, under
//      the updater key both tags' src-tauri/tauri.conf.json carry (installed apps trust the old one);
//   4. latest.json names this version, the previous release's platforms, URLs under this tag,
//      signatures equal to the `.sig` assets, and the draft's notes (what installed apps show);
//   5. `gh attestation verify` ties every installer to release.yml running at refs/tags/<tag>.
// `--no-download` re-checks what an earlier run saved, e.g. after deliberately tampering with a
// file. `--allow-asset-change` accepts an intentional change to the asset set or platforms, after
// confirming the new names against release.yml's expected list. Exits 1 on the first failed group.
// Every pass/fail decision lives in release-verification.mjs, where it is tested; this file only
// gathers the inputs with `gh` and `git`, reads the files, and prints.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import {
  checkAssetSet,
  checkFeed,
  checkSignatures,
  checkSums,
  checkUpdaterKeys,
  draftBody,
  expectedAssetNames,
} from "./release-verification.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const usage =
  "usage: verify-release.mjs <vX.Y.Z> <previous vX.Y.Z> [--repo owner/name] [--dir path] [--no-download] [--allow-asset-change]";

let parsed;
try {
  parsed = parseArgs({
    strict: true,
    allowPositionals: true,
    options: {
      repo: { type: "string", default: "giostriquer/on-n-off" },
      dir: { type: "string", default: join(repoRoot, ".tmp", "release-check") },
      "no-download": { type: "boolean", default: false },
      "allow-asset-change": { type: "boolean", default: false },
    },
  });
} catch (error) {
  console.error(`${error.message}\n${usage}`);
  process.exit(2);
}
const { values, positionals } = parsed;
const [tag, previousTag] = positionals;
if (positionals.length !== 2 || !/^v\d+\.\d+\.\d+$/.test(tag) || !/^v\d+\.\d+\.\d+$/.test(previousTag)) {
  console.error(usage);
  process.exit(2);
}
const repo = values.repo;
const root = resolve(values.dir);
const version = tag.slice(1);
const previousVersion = previousTag.slice(1);
const allowChange = values["allow-asset-change"];

let failed = 0;
const print = ({ ok, message, detail }) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${message}${!ok && detail ? `\n       ${detail}` : ""}`);
  if (!ok) failed += 1;
};
/** Print one group's checks and accepted differences; stop at the first group that failed. */
const report = (name, { checks, warnings }) => {
  if (warnings.length > 0) {
    console.log(`warn ${name} differs from ${previousTag}, accepted by --allow-asset-change; confirm each against release.yml:`);
    for (const warning of warnings) console.log(`warn   ${warning}`);
  }
  checks.forEach(print);
  if (failed > 0) {
    console.error(`\n${name} failed; ${failed} problem(s). The release is NOT verified.`);
    process.exit(1);
  }
};
const gh = (args, options = {}) => execFileSync("gh", args, { encoding: "utf8", ...options });

// Inputs: this release in full and its draft body; the previous one's asset names and latest.json.
const dir = join(root, tag);
const previousDir = join(root, previousTag);
if (!values["no-download"]) {
  for (const target of [dir, previousDir]) {
    rmSync(target, { recursive: true, force: true });
    mkdirSync(target, { recursive: true });
  }
  gh(["release", "download", tag, "--repo", repo, "--dir", dir], { stdio: "inherit" });
  writeFileSync(join(root, `${tag}.body.md`), gh(["release", "view", tag, "--repo", repo, "--json", "body", "-q", ".body"]));
  gh(["release", "download", previousTag, "--repo", repo, "--dir", previousDir, "--pattern", "latest.json"], { stdio: "inherit" });
  const names = JSON.parse(gh(["release", "view", previousTag, "--repo", repo, "--json", "assets"])).assets.map((a) => a.name);
  writeFileSync(join(root, `${previousTag}.assets.json`), JSON.stringify(names));
}
const assets = readdirSync(dir).sort();
const text = (path) => readFileSync(path, "utf8");
/** A file the run may lack (an asset removed under --allow-asset-change, a partial --no-download directory): its check reports it. */
const textIfPresent = (path) => (existsSync(path) ? text(path) : "");
const previousAssets = JSON.parse(text(join(root, `${previousTag}.assets.json`)));
const bytesByName = new Map(assets.filter((name) => name !== "SHA256SUMS.txt").map((name) => [name, readFileSync(join(dir, name))]));
const sigTextByName = new Map(assets.filter((name) => name.endsWith(".sig")).map((name) => [name, text(join(dir, name))]));

// 1. Asset set.
report("asset set", checkAssetSet(assets, expectedAssetNames(previousAssets, previousVersion, version), allowChange));

// 2. SHA256SUMS.txt.
report("SHA256SUMS", checkSums(assets.includes("SHA256SUMS.txt") ? text(join(dir, "SHA256SUMS.txt")) : "", bytesByName));

// 3. Minisign signatures under the updater key installed apps already trust.
const configAt = (ref) =>
  execFileSync("git", ["show", `${ref}:src-tauri/tauri.conf.json`], { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
let configs;
try {
  configs = [configAt(previousTag), configAt(tag)];
} catch (error) {
  report("updater key", { checks: [{ ok: false, message: `tauri.conf.json is readable at ${previousTag} and ${tag}`, detail: `${error.message.split("\n")[0]} (git fetch --tags)` }], warnings: [] });
}
const keys = checkUpdaterKeys(...configs);
report("updater key", keys);
report("signatures", checkSignatures(sigTextByName, bytesByName, version, keys.publicKey));

// 4. latest.json.
report(
  "latest.json",
  checkFeed(textIfPresent(join(dir, "latest.json")), textIfPresent(join(previousDir, "latest.json")), {
    repo,
    tag,
    version,
    assets,
    sigTextByName,
    body: draftBody(textIfPresent(join(root, `${tag}.body.md`))),
    allowChange,
  }),
);

// 5. Build provenance: this repo's Release workflow, at this tag, on GitHub-hosted runners.
const attestations = [];
for (const name of assets.filter((name) => /\.(exe|dmg|app\.tar\.gz)$/.test(name))) {
  let error = null;
  try {
    gh(
      [
        "attestation", "verify", join(dir, name),
        "--repo", repo,
        "--signer-workflow", `${repo}/.github/workflows/release.yml`,
        "--source-ref", `refs/tags/${tag}`,
        "--deny-self-hosted-runners",
      ],
      { stdio: "pipe" },
    );
  } catch (caught) {
    error = String(caught.stderr ?? caught.message).trim().split(/\r?\n/).slice(-3).join(" | ");
  }
  attestations.push({ ok: error === null, message: `attestation ${name} (release.yml at refs/tags/${tag})`, detail: error });
}
report("attestations", { checks: attestations, warnings: [] });

console.log(`\n${tag} verified against ${previousTag}.`);
