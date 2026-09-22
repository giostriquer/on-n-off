// Run with `bun test scripts/` (CI's frontend job) or `node --test scripts/*.test.mjs`.
import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  expectedAssetNames,
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
const repoKey = parseUpdaterPublicKey(readFileSync(join(here, "..", "src-tauri", "tauri.conf.json"), "utf8"));

const b64 = (bytes) => Buffer.from(bytes).toString("base64");

/** A throwaway minisign key, its tauri.conf.json text, and a signer producing Tauri `.sig` text. */
function throwawayKey() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(12);
  const keyId = Buffer.from("0102030405060708", "hex");
  const pubFile = `untrusted comment: minisign public key: 0807060504030201\n${b64(Buffer.concat([Buffer.from("Ed"), keyId, raw]))}\n`;
  const conf = JSON.stringify({ plugins: { updater: { pubkey: b64(pubFile) } } });
  const signFile = (data, trustedComment) => {
    const signature = sign(null, createHash("blake2b512").update(data).digest(), privateKey);
    const global = sign(null, Buffer.concat([signature, Buffer.from(trustedComment)]), privateKey);
    const text = `untrusted comment: signature from tauri secret key\n${b64(Buffer.concat([Buffer.from("ED"), keyId, signature]))}\ntrusted comment: ${trustedComment}\n${b64(global)}\n`;
    return b64(text);
  };
  return { conf, signFile };
}

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
  const sig = parseMinisignSignature(signer.signFile(data, "timestamp:1\tfile:on-n-off_1.0.0_x64-setup.exe"));
  assert.deepEqual(verifyMinisign(sig, data, key), { madeWithKey: true, signsFile: true, trustedCommentSigned: true });
  assert.equal(verifyMinisign(sig, Buffer.from("installer bytes!"), key).signsFile, false);
  const other = verifyMinisign(sig, data, repoKey);
  assert.equal(other.madeWithKey, false);
  assert.equal(other.signsFile, false);
});

test("malformed signatures and keys are refused rather than half-read", () => {
  assert.throws(() => parseMinisignSignature(""), /not a minisign signature/);
  assert.throws(() => parseMinisignSignature(b64("untrusted comment: x\nshort\n")), /not a minisign signature/);
  assert.throws(() => parseUpdaterPublicKey("{}"), /no plugins.updater.pubkey/);
  assert.throws(() => parseUpdaterPublicKey(JSON.stringify({ plugins: { updater: { pubkey: b64("x\ny\n") } } })), /not a minisign/);
});

test("the trusted comment may name the pre-rename macOS bundle but not another version", () => {
  assert.deepEqual(signedFileNames("on-n-off_0.15.0_aarch64.app.tar.gz", "0.15.0"), [
    "on-n-off_0.15.0_aarch64.app.tar.gz",
    "on-n-off.app.tar.gz",
  ]);
  assert.equal(trustedCommentNames("timestamp:1\tfile:on-n-off.app.tar.gz", "on-n-off_0.15.0_aarch64.app.tar.gz", "0.15.0"), true);
  assert.equal(trustedCommentNames("timestamp:1\tfile:on-n-off.app.tar.gz", "on-n-off_0.15.0_x64.app.tar.gz", "0.15.0"), true);
  assert.equal(trustedCommentNames("timestamp:1\tfile:on-n-off_0.14.0_x64-setup.exe", "on-n-off_0.15.0_x64-setup.exe", "0.15.0"), false);
});

test("SHA256SUMS lines parse with CRLF endings and the binary-mode marker", () => {
  const a = "a".repeat(64);
  const b = "B".repeat(64);
  const sums = parseSums(`${a}  latest.json\r\n${b} *on-n-off_0.15.0_aarch64.dmg\r\nnot a line\r\n`);
  assert.deepEqual([...sums], [["latest.json", a], ["on-n-off_0.15.0_aarch64.dmg", b.toLowerCase()]]);
});

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
});
