// Run with `bun test scripts/` (CI's frontend job) or `node --test scripts/*.test.mjs`.
import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  checkAssetSet,
  checkFeed,
  checkSignatures,
  checkSums,
  checkUpdaterKeys,
  draftBody,
  expectedAssetNames,
  failuresOf,
  notesMatch,
  parseMinisignSignature,
  parseSums,
  parseUpdaterPublicKey,
  signedFileNames,
  trustedCommentNames,
  verifyMinisign,
} from "./release-verification.mjs";

const here = dirname(fileURLToPath(import.meta.url));
/** The Windows installer signature published with v0.14.0, as the Release workflow wrote it. */
const realSig = readFileSync(join(here, "fixtures", "on-n-off_0.14.0_x64-setup.exe.sig"), "utf8");
const repoConf = readFileSync(join(here, "..", "src-tauri", "tauri.conf.json"), "utf8");
const repoKey = parseUpdaterPublicKey(repoConf);

const b64 = (bytes) => Buffer.from(bytes).toString("base64");
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

/** A minisign signature file as Tauri publishes it, from its raw parts. */
function minisignText(signatureLine, trustedLine, globalSignature) {
  return b64(`untrusted comment: signature from tauri secret key\n${b64(signatureLine)}\n${trustedLine}\n${b64(globalSignature)}\n`);
}

/**
 * A throwaway minisign key: its tauri.conf.json text, and a signer producing Tauri `.sig` text.
 * `algorithm` "ED" signs the BLAKE2b-512 of the data (what Tauri writes), "Ed" the data itself.
 */
function throwawayKey(keyId = Buffer.from("0102030405060708", "hex")) {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(12);
  const pubFile = `untrusted comment: minisign public key\n${b64(Buffer.concat([Buffer.from("Ed"), keyId, raw]))}\n`;
  const conf = JSON.stringify({ plugins: { updater: { pubkey: b64(pubFile) } } });
  const signParts = (data, trustedComment, algorithm = "ED") => {
    const message = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
    const signature = sign(null, message, privateKey);
    const global = sign(null, Buffer.concat([signature, Buffer.from(trustedComment)]), privateKey);
    return { signatureLine: Buffer.concat([Buffer.from(algorithm), keyId, signature]), global };
  };
  const signFile = (data, trustedComment, algorithm = "ED") => {
    const { signatureLine, global } = signParts(data, trustedComment, algorithm);
    return minisignText(signatureLine, `trusted comment: ${trustedComment}`, global);
  };
  return { conf, signFile, signParts };
}

/* -------------------------------------------------------------------------------------------- */
/* Minisign                                                                                     */
/* -------------------------------------------------------------------------------------------- */

test("a real release signature decodes, was made with the repo's updater key and signs its trusted comment", () => {
  const sig = parseMinisignSignature(realSig);
  assert.equal(sig.algorithm, "ED");
  assert.ok(repoKey.keyId.equals(sig.keyId));
  assert.equal(sig.trustedComment, "timestamp:1790048769\tfile:on-n-off_0.14.0_x64-setup.exe");
  const result = verifyMinisign(sig, Buffer.from("not the installer"), repoKey);
  assert.equal(result.madeWithKey, true);
  assert.equal(result.trustedCommentSigned, true);
  assert.equal(result.signsFile, false, "the signature covers the installer, not these bytes");
});

test("an edited trusted comment no longer verifies", () => {
  const sig = parseMinisignSignature(realSig);
  const edited = { ...sig, trustedComment: sig.trustedComment.replace("0.14.0", "0.15.0") };
  assert.equal(verifyMinisign(edited, Buffer.alloc(0), repoKey).trustedCommentSigned, false);
});

test("a file signature holds for the signed bytes only, and only under the signing key", () => {
  const signer = throwawayKey();
  const key = parseUpdaterPublicKey(signer.conf);
  const data = Buffer.from("installer bytes");
  const sig = parseMinisignSignature(signer.signFile(data, "timestamp:1\tfile:webapp_1.0.0_x64-setup.exe"));
  assert.deepEqual(verifyMinisign(sig, data, key), { madeWithKey: true, signsFile: true, trustedCommentSigned: true });
  assert.equal(verifyMinisign(sig, Buffer.from("installer bytes!"), key).signsFile, false);
  const other = verifyMinisign(sig, data, repoKey);
  assert.equal(other.madeWithKey, false);
  assert.equal(other.signsFile, false);
});

test("a copied key id is not proof: a forged signature claims the repo's key id and still fails", () => {
  const forger = throwawayKey(repoKey.keyId);
  const data = Buffer.from("installer bytes");
  const sig = parseMinisignSignature(forger.signFile(data, "timestamp:1\tfile:on-n-off_0.15.0_x64-setup.exe"));
  assert.deepEqual(verifyMinisign(sig, data, repoKey), { madeWithKey: true, signsFile: false, trustedCommentSigned: false });
});

test("legacy Ed signs the raw file and ED its BLAKE2b-512; relabelling either one breaks it", () => {
  const signer = throwawayKey();
  const key = parseUpdaterPublicKey(signer.conf);
  const data = Buffer.from("installer bytes");
  const comment = "timestamp:1\tfile:webapp_1.0.0_x64-setup.exe";
  for (const [algorithm, relabel] of [["Ed", "ED"], ["ED", "Ed"]]) {
    const { signatureLine, global } = signer.signParts(data, comment, algorithm);
    const genuine = parseMinisignSignature(minisignText(signatureLine, `trusted comment: ${comment}`, global));
    assert.equal(genuine.algorithm, algorithm);
    assert.equal(verifyMinisign(genuine, data, key).signsFile, true, `${algorithm} verifies`);
    const relabelled = Buffer.concat([Buffer.from(relabel), signatureLine.subarray(2)]);
    const wrong = parseMinisignSignature(minisignText(relabelled, `trusted comment: ${comment}`, global));
    assert.equal(verifyMinisign(wrong, data, key).signsFile, false, `${algorithm} relabelled ${relabel} fails`);
  }
});

test("each malformed signature or key part is refused with its own reason", () => {
  const signer = throwawayKey();
  const comment = "timestamp:1\tfile:webapp_1.0.0_x64-setup.exe";
  const { signatureLine, global } = signer.signParts(Buffer.from("data"), comment);
  const trusted = `trusted comment: ${comment}`;
  const rows = [
    ["algorithm XX", minisignText(Buffer.concat([Buffer.from("XX"), signatureLine.subarray(2)]), trusted, global), /unsupported minisign algorithm "XX"/],
    ["73-byte ED line", minisignText(signatureLine.subarray(0, 73), trusted, global), /73 bytes, not minisign's 74/],
    ["no trusted comment", minisignText(signatureLine, comment, global), /no trusted comment/],
    ["63-byte global signature", minisignText(signatureLine, trusted, global.subarray(0, 63)), /signature is 63 bytes, not 64/],
  ];
  for (const [name, text, reason] of rows) assert.throws(() => parseMinisignSignature(text), reason, name);
  assert.doesNotThrow(() => parseMinisignSignature(minisignText(signatureLine, trusted, global)), "the unmodified parts parse");

  const keyLine = Buffer.from(JSON.parse(signer.conf).plugins.updater.pubkey, "base64").toString("utf8").split("\n")[1];
  const keyBytes = Buffer.from(keyLine, "base64");
  const conf = (bytes) => JSON.stringify({ plugins: { updater: { pubkey: b64(`untrusted comment: k\n${b64(bytes)}\n`) } } });
  assert.throws(() => parseUpdaterPublicKey(conf(Buffer.concat([Buffer.from("XX"), keyBytes.subarray(2)]))), /not an Ed25519 minisign key/);
  assert.throws(() => parseUpdaterPublicKey(conf(Buffer.concat([keyBytes, Buffer.from([0])]))), /43 bytes, not a minisign key's 42/);
  assert.throws(() => parseUpdaterPublicKey("{}"), /no plugins.updater.pubkey/);
  assert.doesNotThrow(() => parseUpdaterPublicKey(conf(keyBytes)), "the unmodified key parses");
});

test("a trusted comment names exactly its asset, or the macOS bundle as Tauri signed it before renaming", () => {
  assert.deepEqual(signedFileNames("on-n-off_0.15.0_aarch64.app.tar.gz", "0.15.0"), [
    "on-n-off_0.15.0_aarch64.app.tar.gz",
    "on-n-off.app.tar.gz",
  ]);
  const exe = "on-n-off_0.15.0_x64-setup.exe";
  assert.equal(trustedCommentNames(`timestamp:1\tfile:${exe}`, exe, "0.15.0"), true);
  assert.equal(trustedCommentNames("timestamp:1\tfile:on-n-off_0.14.0_x64-setup.exe", exe, "0.15.0"), false, "another version");
  assert.equal(trustedCommentNames(`timestamp:1\tfile:${exe}.bak`, exe, "0.15.0"), false, "a longer name");
  assert.equal(trustedCommentNames("timestamp:1\tfile:on-n-off_0.15.0_x64-setup", exe, "0.15.0"), false, "a prefix");
  assert.equal(trustedCommentNames(`timestamp:1\tfile:x${exe}`, exe, "0.15.0"), false, "a longer name ending in it");
  // The macOS name carries no version, so a previous release's bundle and its .sig, replayed under
  // this release's name, pass this check. Only the attestation group (the build ran at this tag)
  // stops that replay.
  assert.equal(trustedCommentNames("timestamp:1\tfile:on-n-off.app.tar.gz", "on-n-off_0.15.0_aarch64.app.tar.gz", "0.15.0"), true);
});

/* -------------------------------------------------------------------------------------------- */
/* SHA256SUMS                                                                                   */
/* -------------------------------------------------------------------------------------------- */

test("SHA256SUMS lines parse with CRLF or LF endings and the binary-mode marker", () => {
  const a = "a".repeat(64);
  const b = "B".repeat(64);
  const c = "c".repeat(64);
  const sums = parseSums(`${a}  latest.json\r\n${b} *on-n-off_0.15.0_aarch64.dmg\r\n${c}  LICENSE\n\r\n`);
  assert.deepEqual([...sums], [["latest.json", a], ["on-n-off_0.15.0_aarch64.dmg", b.toLowerCase()], ["LICENSE", c]]);
});

test("a SHA256SUMS line that sha256sum -c would flag is refused, and so is a name listed twice", () => {
  const hex = "a".repeat(64);
  const rows = [
    ["a 65-hex hash", `${"a".repeat(65)}  LICENSE`, /line 1 is not/],
    ["a garbage prefix", `zz${hex}  LICENSE`, /line 1 is not/],
    ["a SHA-512 line", `${"a".repeat(128)}  LICENSE`, /line 1 is not/],
    ["prose", "not a line", /line 1 is not/],
    ["a duplicate", `${hex}  LICENSE\n${"b".repeat(64)}  LICENSE`, /lists LICENSE twice/],
  ];
  for (const [name, text, reason] of rows) assert.throws(() => parseSums(text), reason, name);
});

/* -------------------------------------------------------------------------------------------- */
/* Release groups                                                                               */
/* -------------------------------------------------------------------------------------------- */

test("the expected asset set is the previous one renamed, sorted", () => {
  assert.deepEqual(
    expectedAssetNames(["on-n-off_0.14.0_x64-setup.exe", "LICENSE", "on-n-off_0.14.0_x64-setup.exe.sig"], "0.14.0", "0.15.0"),
    ["LICENSE", "on-n-off_0.15.0_x64-setup.exe", "on-n-off_0.15.0_x64-setup.exe.sig"],
  );
});

test("release notes compare across line endings and differ on any edit", () => {
  assert.equal(notesMatch("## What's Changed\n* one", "## What's Changed\r\n* one"), true);
  assert.equal(notesMatch("## What's Changed\n* one", "## What's Changed\n* one\n* two"), false);
  assert.equal(notesMatch(undefined, ""), true);
  assert.equal(draftBody("## What's Changed\r\n* one\r\n"), "## What's Changed\n* one", "normalised, then its trailing newline dropped");
});

test("the updater key must be the one the previous tag carried", () => {
  const other = throwawayKey();
  assert.deepEqual(failuresOf(checkUpdaterKeys(repoConf, repoConf)), []);
  assert.deepEqual(failuresOf(checkUpdaterKeys(repoConf, other.conf)), ["the new tag keeps the updater key installed apps trust"]);
  assert.deepEqual(failuresOf(checkUpdaterKeys("{}", repoConf)), ["the updater key is readable at both tags"]);
});

const REPO = "acme/webapp";
const TAG = "v1.1.0";
const VERSION = "1.1.0";
const EXE = "webapp_1.1.0_x64-setup.exe";
const DMG = "webapp_1.1.0_aarch64.dmg";
const PREVIOUS_ASSETS = [
  "LICENSE",
  "SHA256SUMS.txt",
  "latest.json",
  "webapp_1.0.0_aarch64.dmg",
  "webapp_1.0.0_x64-setup.exe",
  "webapp_1.0.0_x64-setup.exe.sig",
];
const NOTES = "## What's Changed\n* one change";
const releaseSigner = throwawayKey();
const releaseKey = parseUpdaterPublicKey(releaseSigner.conf);

/**
 * A complete, valid release signed with a throwaway key. `tamper` edits the files, the feed and the
 * draft body before SHA256SUMS.txt is written; `tamperSums` edits that text afterwards.
 */
function buildRelease({ tamper = () => {}, tamperSums = (text) => text, allowChange = false } = {}) {
  const files = new Map([
    ["LICENSE", Buffer.from("license text")],
    [DMG, Buffer.from("disk image bytes")],
    [EXE, Buffer.from("installer bytes")],
  ]);
  const sigs = new Map([[`${EXE}.sig`, releaseSigner.signFile(files.get(EXE), `timestamp:1\tfile:${EXE}`)]]);
  const release = {
    files,
    sigs,
    latest: {
      version: VERSION,
      notes: NOTES,
      pub_date: "2026-09-22T00:00:00Z",
      platforms: { "windows-x86_64-nsis": { url: `https://github.com/${REPO}/releases/download/${TAG}/${EXE}`, signature: sigs.get(`${EXE}.sig`) } },
    },
    previousLatest: { version: "1.0.0", platforms: { "windows-x86_64-nsis": {} } },
    body: `${NOTES.replaceAll("\n", "\r\n")}\r\n`,
  };
  tamper(release);
  const bytesByName = new Map(release.files);
  for (const [name, text] of release.sigs) bytesByName.set(name, Buffer.from(text));
  bytesByName.set("latest.json", Buffer.from(JSON.stringify(release.latest)));
  const sums = [...bytesByName].map(([name, bytes]) => `${sha256(bytes)}  ${name}`).join("\r\n");
  const assets = [...bytesByName.keys(), "SHA256SUMS.txt"].sort();
  const expected = expectedAssetNames(PREVIOUS_ASSETS, "1.0.0", VERSION);
  return [
    checkAssetSet(assets, expected, allowChange),
    checkSums(tamperSums(sums), bytesByName),
    checkSignatures(release.sigs, bytesByName, VERSION, releaseKey),
    checkFeed(JSON.stringify(release.latest), JSON.stringify(release.previousLatest), {
      repo: REPO,
      tag: TAG,
      version: VERSION,
      assets,
      sigTextByName: release.sigs,
      body: draftBody(release.body),
      allowChange,
    }),
  ];
}

test("a valid release passes every group, and each tamper fails its own check", () => {
  assert.deepEqual(failuresOf(...buildRelease()), [], "the untouched release is valid, so the rows below are not vacuous");
  const rows = [
    ["an extra asset", { tamper: (r) => r.files.set("webapp_1.1.0_extra.zip", Buffer.from("x")) }, "unexpected asset webapp_1.1.0_extra.zip"],
    ["a missing asset", { tamper: (r) => r.files.delete(DMG) }, `missing asset ${DMG}`],
    [
      "swapped SUMS hashes",
      {
        tamperSums: (text) => {
          const lines = text.split("\r\n");
          const [license, dmg] = [lines.findIndex((l) => l.endsWith("  LICENSE")), lines.findIndex((l) => l.endsWith(`  ${DMG}`))];
          const hash = (i) => lines[i].slice(0, 64);
          [lines[license], lines[dmg]] = [`${hash(dmg)}  LICENSE`, `${hash(license)}  ${DMG}`];
          return lines.join("\r\n");
        },
      },
      "sha256 LICENSE",
    ],
    ["a missing SUMS line", { tamperSums: (text) => text.split("\r\n").filter((l) => !l.endsWith("  LICENSE")).join("\r\n") }, "SHA256SUMS.txt lists every other asset, and nothing else"],
    ["a .sig of another asset", { tamper: (r) => r.sigs.set(`${EXE}.sig`, releaseSigner.signFile(r.files.get(DMG), `timestamp:1\tfile:${EXE}`)) }, `${EXE}.sig signs ${EXE}`],
    [
      "an edited trusted comment",
      {
        tamper: (r) => {
          const lines = Buffer.from(r.sigs.get(`${EXE}.sig`), "base64").toString("utf8").split("\n");
          lines[2] = lines[2].replace("timestamp:1", "timestamp:2");
          r.sigs.set(`${EXE}.sig`, b64(lines.join("\n")));
        },
      },
      `${EXE}.sig trusted comment is signed`,
    ],
    [
      "a feed URL under the previous tag",
      { tamper: (r) => (r.latest.platforms["windows-x86_64-nsis"].url = `https://github.com/${REPO}/releases/download/v1.0.0/${EXE}`) },
      `windows-x86_64-nsis url points at an asset of ${TAG}`,
    ],
    [
      "a feed signature that is not the .sig",
      { tamper: (r) => (r.latest.platforms["windows-x86_64-nsis"].signature = releaseSigner.signFile(r.files.get(EXE), `timestamp:9\tfile:${EXE}`)) },
      `windows-x86_64-nsis signature equals ${EXE}.sig`,
    ],
    ["changed notes", { tamper: (r) => (r.body = `${NOTES}\n* a line added after publish-draft\n`) }, "latest.json notes match the release body (installed apps show these)"],
  ];
  for (const [name, options, failure] of rows) {
    const failures = failuresOf(...buildRelease(options));
    assert.ok(failures.includes(failure), `${name}: expected "${failure}", got ${JSON.stringify(failures)}`);
  }
});

test("--allow-asset-change relaxes only the set check, and names each difference", () => {
  const extra = { tamper: (r) => r.files.set("webapp_1.1.0_extra.zip", Buffer.from("x")) };
  const [set, ...rest] = buildRelease({ ...extra, allowChange: true });
  assert.deepEqual(failuresOf(set, ...rest), []);
  assert.deepEqual(set.warnings, ["unexpected asset webapp_1.1.0_extra.zip"]);

  const [missingSet] = buildRelease({ tamper: (r) => r.files.delete(DMG), allowChange: true });
  assert.deepEqual(missingSet.warnings, [`missing asset ${DMG}`], "a missing installer is its own line");

  const forged = buildRelease({
    tamper: (r) => r.sigs.set(`${EXE}.sig`, releaseSigner.signFile(r.files.get(DMG), `timestamp:1\tfile:${EXE}`)),
    allowChange: true,
  });
  assert.ok(failuresOf(...forged).includes(`${EXE}.sig signs ${EXE}`), "a bad signature still fails with the flag");

  const platforms = buildRelease({ tamper: (r) => (r.previousLatest.platforms["darwin-aarch64"] = {}), allowChange: true });
  assert.deepEqual(platforms[3].warnings, ["missing platform darwin-aarch64"]);
  const strict = buildRelease({ tamper: (r) => (r.previousLatest.platforms["darwin-aarch64"] = {}) });
  assert.ok(failuresOf(...strict).includes("latest.json platforms match the previous release"));
});
