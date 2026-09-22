#!/usr/bin/env node
// Verifies a GitHub release of on-n-off before (or after) it is published.
//
//   bun scripts/verify-release.mjs <vX.Y.Z> <previous vP.Q.R>
//       [--repo owner/name] [--dir .tmp/release-check] [--no-download] [--allow-asset-change]
//
// Downloads the release's assets and body with `gh` (drafts included), plus the previous release's
// asset names and latest.json, and checks:
//   1. the asset set is the previous release's, renamed to the new version;
//   2. SHA256SUMS.txt lists every other asset, and each hash matches;
//   3. every `.sig` is a minisign signature (Ed25519 over BLAKE2b-512) for the file it names, under
//      the updater key both tags' src-tauri/tauri.conf.json carry (installed apps trust the old one);
//   4. latest.json names this version, the previous release's platforms, URLs under this tag,
//      signatures equal to the `.sig` assets, and the draft's notes (what installed apps show);
//   5. `gh attestation verify` ties every installer to release.yml running at refs/tags/<tag>.
// `--no-download` re-checks what an earlier run saved, e.g. after deliberately tampering with a
// file. `--allow-asset-change` accepts an intentional change to the asset set or platforms, after
// confirming the new names against release.yml's expected list. Exits 1 on the first failed group.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import {
  expectedAssetNames,
  notesMatch,
  parseMinisignSignature,
  parseSums,
  parseUpdaterPublicKey,
  trustedCommentNames,
  verifyMinisign,
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

const failures = [];
const check = (ok, message, detail) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${message}${!ok && detail ? `\n       ${detail}` : ""}`);
  if (!ok) failures.push(message);
};
const attempt = (message, work) => {
  try {
    work();
  } catch (error) {
    check(false, message, error.message);
  }
};
const finishGroup = (name) => {
  if (failures.length > 0) {
    console.error(`\n${name} failed; ${failures.length} problem(s). The release is NOT verified.`);
    process.exit(1);
  }
};
const gh = (args, options = {}) => execFileSync("gh", args, { encoding: "utf8", ...options });

// Inputs: this release in full, the previous one's asset names and latest.json.
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
const previousAssets = JSON.parse(readFileSync(join(root, `${previousTag}.assets.json`), "utf8"));
const read = (name) => readFileSync(join(dir, name));

// 1. Asset set.
const expected = expectedAssetNames(previousAssets, previousVersion, version);
const sameSet = JSON.stringify(assets) === JSON.stringify(expected);
if (sameSet || !values["allow-asset-change"]) {
  check(sameSet, `asset set matches ${previousTag} renamed (${assets.length} assets)`);
  for (const name of expected.filter((name) => !assets.includes(name))) check(false, `missing asset ${name}`);
  for (const name of assets.filter((name) => !expected.includes(name))) check(false, `unexpected asset ${name}`);
} else {
  console.log(`warn asset set differs from ${previousTag}; accepted by --allow-asset-change: ${assets.join(", ")}`);
}
finishGroup("Asset set");

// 2. SHA256SUMS.txt.
attempt("SHA256SUMS.txt is readable", () => {
  const sums = parseSums(read("SHA256SUMS.txt").toString("utf8"));
  const summed = assets.filter((name) => name !== "SHA256SUMS.txt");
  check(JSON.stringify([...sums.keys()].sort()) === JSON.stringify(summed), "SHA256SUMS.txt lists every other asset");
  for (const name of summed) {
    check(sums.get(name) === createHash("sha256").update(read(name)).digest("hex"), `sha256 ${name}`);
  }
});
finishGroup("SHA256SUMS");

// 3. Minisign signatures under the updater key installed apps already trust.
const configAt = (ref) =>
  execFileSync("git", ["show", `${ref}:src-tauri/tauri.conf.json`], { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
let publicKey;
attempt(`updater key is read from ${previousTag} and ${tag} (git fetch --tags if a tag is missing)`, () => {
  const trusted = parseUpdaterPublicKey(configAt(previousTag));
  const current = parseUpdaterPublicKey(configAt(tag));
  check(trusted.encoded === current.encoded, `${tag} keeps the updater key installed ${previousTag} apps trust`);
  publicKey = trusted;
});
finishGroup("Updater key");
for (const sigName of assets.filter((name) => name.endsWith(".sig"))) {
  const target = sigName.slice(0, -".sig".length);
  attempt(`${sigName} is a minisign signature of ${target}`, () => {
    const signature = parseMinisignSignature(read(sigName).toString("utf8"));
    const result = verifyMinisign(signature, read(target), publicKey);
    check(result.madeWithKey, `${sigName} was made with the updater key`);
    check(result.signsFile, `${sigName} signs ${target}`);
    check(result.trustedCommentSigned, `${sigName} trusted comment is signed`);
    check(trustedCommentNames(signature.trustedComment, target, version), `${sigName} trusted comment names ${target}`);
  });
}
finishGroup("Signatures");

// 4. latest.json.
attempt("latest.json is readable", () => {
  const latest = JSON.parse(read("latest.json").toString("utf8"));
  const previousLatest = JSON.parse(readFileSync(join(previousDir, "latest.json"), "utf8"));
  check(latest.version === version, `latest.json version is ${version}`);
  check(!Number.isNaN(Date.parse(latest.pub_date)), "latest.json pub_date is a date");
  const platforms = Object.keys(latest.platforms ?? {}).sort();
  const samePlatforms = JSON.stringify(platforms) === JSON.stringify(Object.keys(previousLatest.platforms ?? {}).sort());
  if (samePlatforms || !values["allow-asset-change"]) {
    check(samePlatforms, `latest.json platforms match ${previousTag}: ${platforms.join(", ")}`);
  } else {
    console.log(`warn latest.json platforms differ from ${previousTag}; accepted by --allow-asset-change: ${platforms.join(", ")}`);
  }
  const prefix = `https://github.com/${repo}/releases/download/${tag}/`;
  for (const [platform, entry] of Object.entries(latest.platforms ?? {})) {
    const asset = String(entry?.url ?? "").startsWith(prefix) ? entry.url.slice(prefix.length) : null;
    check(asset !== null && assets.includes(asset), `${platform} url points at an asset of ${tag}`);
    if (asset && assets.includes(`${asset}.sig`)) {
      check(String(entry.signature ?? "").trim() === read(`${asset}.sig`).toString("utf8").trim(), `${platform} signature equals ${asset}.sig`);
    } else {
      check(false, `${platform} has a .sig asset`);
    }
  }
  const body = readFileSync(join(root, `${tag}.body.md`), "utf8");
  check(
    notesMatch(latest.notes, body.replace(/\n$/, "")),
    "latest.json notes match the release body (installed apps show these)",
    "the draft notes changed after publish-draft ran: re-run that job so latest.json carries them",
  );
});
finishGroup("latest.json");

// 5. Build provenance: this repo's Release workflow, at this tag, on GitHub-hosted runners.
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
  check(error === null, `attestation ${name} (release.yml at refs/tags/${tag})`, error);
}
finishGroup("Attestations");

console.log(`\n${tag} verified against ${previousTag}.`);
