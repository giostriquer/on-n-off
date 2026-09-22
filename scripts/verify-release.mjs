#!/usr/bin/env node
// Verifies a GitHub release of on-n-off before (or after) it is published.
//
//   node scripts/verify-release.mjs <tag> <previous-tag> [--repo owner/name] [--dir .tmp/release-check]
//                                   [--config src-tauri/tauri.conf.json] [--no-download]
//
// Downloads both releases' assets with `gh release download` (drafts included) and checks:
//   1. the asset set is the previous release's, renamed to the new version;
//   2. SHA256SUMS.txt lists every other asset, and each hash matches;
//   3. every `.sig` is a valid minisign signature (Ed25519 over BLAKE2b-512) under the updater
//      public key in src-tauri/tauri.conf.json, trusted comment included, for the file it names;
//   4. latest.json names this version, the previous release's platforms, URLs under this tag,
//      and signatures byte-identical to the `.sig` assets;
//   5. `gh attestation verify` accepts every installer.
// Exits non-zero on the first failed check group, after printing what failed.

import { execFileSync } from "node:child_process";
import { createHash, createPublicKey, verify } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  const [value] = args.splice(index, 2).slice(1);
  return value;
};
const repo = option("--repo", "giostriquer/on-n-off");
const root = resolve(option("--dir", ".tmp/release-check"));
const config = resolve(option("--config", "src-tauri/tauri.conf.json"));
// Re-checks what an earlier run downloaded, e.g. after deliberately tampering with a file.
const noDownload = args.includes("--no-download") && args.splice(args.indexOf("--no-download"), 1);
const [tag, previousTag] = args;
if (!/^v\d+\.\d+\.\d+$/.test(tag ?? "") || !/^v\d+\.\d+\.\d+$/.test(previousTag ?? "")) {
  console.error("usage: verify-release.mjs <vX.Y.Z> <previous vX.Y.Z> [--repo owner/name] [--dir path] [--config tauri.conf.json]");
  process.exit(2);
}
const version = tag.slice(1);
const previousVersion = previousTag.slice(1);

const failures = [];
const check = (ok, message) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${message}`);
  if (!ok) failures.push(message);
};
const finishGroup = (name) => {
  if (failures.length > 0) {
    console.error(`\n${name} failed; ${failures.length} problem(s). The release is NOT verified.`);
    process.exit(1);
  }
};

function download(releaseTag) {
  const dir = join(root, releaseTag);
  if (noDownload) return dir;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  execFileSync("gh", ["release", "download", releaseTag, "--repo", repo, "--dir", dir], { stdio: "inherit" });
  return dir;
}

const dir = download(tag);
const previousDir = download(previousTag);
const assets = readdirSync(dir).sort();
const previousAssets = readdirSync(previousDir).sort();
const read = (name) => readFileSync(join(dir, name));

// 1. Asset set.
const expected = previousAssets.map((name) => name.replaceAll(previousVersion, version)).sort();
check(JSON.stringify(assets) === JSON.stringify(expected), `asset set matches ${previousTag} renamed (${assets.length} assets)`);
for (const name of expected.filter((name) => !assets.includes(name))) check(false, `missing asset ${name}`);
for (const name of assets.filter((name) => !expected.includes(name))) check(false, `unexpected asset ${name}`);
finishGroup("Asset set");

// 2. SHA256SUMS.txt.
const sums = new Map(
  read("SHA256SUMS.txt")
    .toString("utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      const [hash, ...rest] = line.trim().split(/\s+/);
      return [rest.join(" ").replace(/^\*/, ""), hash.toLowerCase()];
    }),
);
const summed = assets.filter((name) => name !== "SHA256SUMS.txt");
check(JSON.stringify([...sums.keys()].sort()) === JSON.stringify(summed), "SHA256SUMS.txt lists every other asset");
for (const name of summed) {
  const actual = createHash("sha256").update(read(name)).digest("hex");
  check(sums.get(name) === actual, `sha256 ${name}`);
}
finishGroup("SHA256SUMS");

// 3. Minisign signatures under the updater key.
const pubkeyText = Buffer.from(JSON.parse(readFileSync(config, "utf8")).plugins.updater.pubkey, "base64").toString("utf8");
const pubkeyBytes = Buffer.from(pubkeyText.split(/\r?\n/)[1].trim(), "base64");
check(pubkeyBytes.length === 42 && pubkeyBytes.subarray(0, 2).toString() === "Ed", "updater public key is a minisign Ed25519 key");
const keyId = pubkeyBytes.subarray(2, 10);
const publicKey = createPublicKey({
  key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), pubkeyBytes.subarray(10)]),
  format: "der",
  type: "spki",
});
for (const sigName of assets.filter((name) => name.endsWith(".sig"))) {
  const target = sigName.slice(0, -".sig".length);
  const lines = Buffer.from(read(sigName).toString("utf8").trim(), "base64").toString("utf8").split(/\r?\n/);
  const signature = Buffer.from(lines[1].trim(), "base64");
  const trusted = lines[2].replace(/^trusted comment: /, "");
  const globalSignature = Buffer.from(lines[3].trim(), "base64");
  const algorithm = signature.subarray(0, 2).toString();
  const message = algorithm === "ED" ? createHash("blake2b512").update(read(target)).digest() : read(target);
  check(signature.length === 74 && keyId.equals(signature.subarray(2, 10)), `${sigName} was made with the updater key`);
  check(verify(null, message, publicKey, signature.subarray(10)), `${sigName} signs ${target}`);
  check(verify(null, Buffer.concat([signature.subarray(10), Buffer.from(trusted)]), publicKey, globalSignature), `${sigName} trusted comment is signed`);
  // Tauri signs the macOS bundle as `on-n-off.app.tar.gz`, before build-bundle.ps1 adds the version
  // and architecture to its name; the updater does not read this field.
  const signedAs = [target, target.replace(`_${version}_aarch64`, "")].map((name) => `file:${name}`);
  check(trusted.split("\t").some((field) => signedAs.includes(field)), `${sigName} trusted comment names ${target}`);
}
finishGroup("Signatures");

// 4. latest.json.
const latest = JSON.parse(read("latest.json").toString("utf8"));
const previousLatest = JSON.parse(readFileSync(join(previousDir, "latest.json"), "utf8"));
check(latest.version === version, `latest.json version is ${version}`);
check(!Number.isNaN(Date.parse(latest.pub_date)), "latest.json pub_date is a date");
check(
  JSON.stringify(Object.keys(latest.platforms).sort()) === JSON.stringify(Object.keys(previousLatest.platforms).sort()),
  `latest.json platforms match ${previousTag}: ${Object.keys(latest.platforms).sort().join(", ")}`,
);
for (const [platform, entry] of Object.entries(latest.platforms)) {
  const prefix = `https://github.com/${repo}/releases/download/${tag}/`;
  const asset = entry.url.startsWith(prefix) ? entry.url.slice(prefix.length) : null;
  check(asset !== null && assets.includes(asset), `${platform} url points at an asset of ${tag}`);
  if (asset) check(entry.signature === read(`${asset}.sig`).toString("utf8"), `${platform} signature equals ${asset}.sig`);
}
finishGroup("latest.json");

// 5. Build provenance.
for (const name of assets.filter((name) => /\.(exe|dmg|app\.tar\.gz)$/.test(name))) {
  let ok = true;
  try {
    execFileSync("gh", ["attestation", "verify", join(dir, name), "--repo", repo], { stdio: "pipe" });
  } catch {
    ok = false;
  }
  check(ok, `attestation ${name}`);
}
finishGroup("Attestations");

console.log(`\n${tag} verified against ${previousTag}.`);
