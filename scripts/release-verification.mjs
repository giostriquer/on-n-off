// Pure checks behind scripts/verify-release.mjs: minisign decoding and verification, SHA256SUMS
// parsing, the expected asset set, and release-notes comparison. Nothing here touches the network,
// the filesystem or `gh`; the CLI gathers the inputs and prints the results.

import { createHash, createPublicKey, verify } from "node:crypto";

/** Ed25519 SubjectPublicKeyInfo prefix, so a raw 32-byte minisign key becomes a Node KeyObject. */
const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

/**
 * The updater public key Tauri keeps in `tauri.conf.json` under `plugins.updater.pubkey`: a base64
 * minisign public-key file whose second line is `Ed` + 8-byte key id + 32-byte Ed25519 key.
 */
export function parseUpdaterPublicKey(tauriConfText) {
  const pubkey = JSON.parse(tauriConfText)?.plugins?.updater?.pubkey;
  if (typeof pubkey !== "string") throw new Error("tauri.conf.json has no plugins.updater.pubkey");
  const line = Buffer.from(pubkey, "base64").toString("utf8").split(/\r?\n/)[1]?.trim() ?? "";
  const bytes = Buffer.from(line, "base64");
  if (bytes.length !== 42 || bytes.subarray(0, 2).toString() !== "Ed") {
    throw new Error("the updater pubkey is not a minisign Ed25519 public key");
  }
  return {
    encoded: pubkey,
    keyId: bytes.subarray(2, 10),
    key: createPublicKey({ key: Buffer.concat([ED25519_SPKI_PREFIX, bytes.subarray(10)]), format: "der", type: "spki" }),
  };
}

/**
 * A Tauri `.sig` asset: base64 of a minisign signature file (untrusted comment, signature line,
 * trusted comment, global signature). The signature line is a 2-byte algorithm (`ED` signs the
 * BLAKE2b-512 of the file, `Ed` the file itself), an 8-byte key id and a 64-byte signature.
 */
export function parseMinisignSignature(sigAssetText) {
  const lines = Buffer.from(sigAssetText.trim(), "base64").toString("utf8").split(/\r?\n/);
  const signature = Buffer.from(lines[1]?.trim() ?? "", "base64");
  if (signature.length !== 74 || !["ED", "Ed"].includes(signature.subarray(0, 2).toString())) {
    throw new Error("not a minisign signature");
  }
  if (!lines[2]?.startsWith("trusted comment: ")) throw new Error("minisign signature has no trusted comment");
  const globalSignature = Buffer.from(lines[3]?.trim() ?? "", "base64");
  if (globalSignature.length !== 64) throw new Error("minisign trusted comment is not signed");
  return {
    algorithm: signature.subarray(0, 2).toString(),
    keyId: signature.subarray(2, 10),
    signature: signature.subarray(10),
    trustedComment: lines[2].slice("trusted comment: ".length),
    globalSignature,
  };
}

/** Every property a release needs from one signature, each as its own yes/no. */
export function verifyMinisign(parsed, fileBytes, publicKey) {
  const message = parsed.algorithm === "ED" ? createHash("blake2b512").update(fileBytes).digest() : fileBytes;
  return {
    madeWithKey: publicKey.keyId.equals(parsed.keyId),
    signsFile: verify(null, message, publicKey.key, parsed.signature),
    trustedCommentSigned: verify(
      null,
      Buffer.concat([parsed.signature, Buffer.from(parsed.trustedComment)]),
      publicKey.key,
      parsed.globalSignature,
    ),
  };
}

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * The names a trusted comment's `file:` may carry for `asset`: the asset itself, or the name Tauri
 * signed before build-bundle.ps1 added `_<version>_<arch>` (`on-n-off.app.tar.gz`).
 */
export function signedFileNames(asset, version) {
  return [asset, asset.replace(new RegExp(`_${escapeRegExp(version)}_[^.]+`), "")];
}

export function trustedCommentNames(trustedComment, asset, version) {
  const accepted = signedFileNames(asset, version).map((name) => `file:${name}`);
  return trustedComment.split("\t").some((field) => accepted.includes(field));
}

/** `SHA256SUMS.txt` lines (`<hex>  <name>`, CRLF from the Windows runner) as name → lowercase hex. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split(/\r?\n/)) {
    const match = /^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$/.exec(line);
    if (match) sums.set(match[2], match[1].toLowerCase());
  }
  return sums;
}

/** The previous release's asset names with its version replaced by the new one, sorted. */
export function expectedAssetNames(previousNames, previousVersion, version) {
  return previousNames.map((name) => name.replaceAll(previousVersion, version)).sort();
}

/** new-update-feed.ps1 copies the draft body into latest.json with CRLF turned into LF. */
export function normalizeNotes(text) {
  return String(text ?? "").replace(/\r\n?/g, "\n");
}

export function notesMatch(latestNotes, draftBody) {
  return normalizeNotes(latestNotes) === normalizeNotes(draftBody);
}
