import { createHash, createPublicKey, verify } from "node:crypto";

const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");
const SIGNATURE_ALGORITHMS = ["ED", "Ed"];

const pass = (message) => ({ ok: true, message });
const fail = (message, detail) => ({ ok: false, message, ...(detail ? { detail } : {}) });
const sameList = (a, b) => JSON.stringify([...a].sort()) === JSON.stringify([...b].sort());
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function failuresOf(...groups) {
  return groups.flatMap((group) => group.checks.filter((check) => !check.ok).map((check) => check.message));
}

export function parseUpdaterPublicKey(tauriConfText) {
  const pubkey = JSON.parse(tauriConfText)?.plugins?.updater?.pubkey;
  if (typeof pubkey !== "string") throw new Error("tauri.conf.json has no plugins.updater.pubkey");
  const line = Buffer.from(pubkey, "base64").toString("utf8").split(/\r?\n/)[1]?.trim() ?? "";
  const bytes = Buffer.from(line, "base64");
  if (bytes.length !== 42) throw new Error(`the updater pubkey is ${bytes.length} bytes, not a minisign key's 42`);
  if (bytes.subarray(0, 2).toString() !== "Ed") throw new Error("the updater pubkey is not an Ed25519 minisign key");
  return {
    encoded: pubkey,
    keyId: bytes.subarray(2, 10),
    key: createPublicKey({ key: Buffer.concat([ED25519_SPKI_PREFIX, bytes.subarray(10)]), format: "der", type: "spki" }),
  };
}

export function parseMinisignSignature(sigAssetText) {
  const lines = Buffer.from(sigAssetText.trim(), "base64").toString("utf8").split(/\r?\n/);
  const signature = Buffer.from(lines[1]?.trim() ?? "", "base64");
  if (signature.length !== 74) throw new Error(`the signature line is ${signature.length} bytes, not minisign's 74`);
  const algorithm = signature.subarray(0, 2).toString();
  if (!SIGNATURE_ALGORITHMS.includes(algorithm)) throw new Error(`unsupported minisign algorithm "${algorithm}"`);
  if (!lines[2]?.startsWith("trusted comment: ")) throw new Error("the signature has no trusted comment");
  const globalSignature = Buffer.from(lines[3]?.trim() ?? "", "base64");
  if (globalSignature.length !== 64) {
    throw new Error(`the trusted comment's signature is ${globalSignature.length} bytes, not 64`);
  }
  return {
    algorithm,
    keyId: signature.subarray(2, 10),
    signature: signature.subarray(10),
    trustedComment: lines[2].slice("trusted comment: ".length),
    globalSignature,
  };
}

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

export function signedFileNames(asset, version) {
  return [asset, asset.replace(new RegExp(`_${escapeRegExp(version)}_[^.]+`), "")];
}

export function trustedCommentNames(trustedComment, asset, version) {
  const accepted = signedFileNames(asset, version).map((name) => `file:${name}`);
  return trustedComment.split("\t").some((field) => accepted.includes(field));
}

export function expectedAssetNames(previousNames, previousVersion, version) {
  return previousNames.map((name) => name.replaceAll(previousVersion, version)).sort();
}

export function checkAssetSet(assets, expected, allowChange) {
  const differences = [
    ...expected.filter((name) => !assets.includes(name)).map((name) => `missing asset ${name}`),
    ...assets.filter((name) => !expected.includes(name)).map((name) => `unexpected asset ${name}`),
  ];
  if (differences.length === 0) {
    return { checks: [pass(`asset set matches the previous release, renamed (${assets.length} assets)`)], warnings: [] };
  }
  if (allowChange) return { checks: [], warnings: differences };
  return { checks: [fail("asset set matches the previous release, renamed"), ...differences.map((d) => fail(d))], warnings: [] };
}

export function parseSums(text) {
  const sums = new Map();
  text.split(/\r?\n/).forEach((line, index) => {
    if (line.trim() === "") return;
    const match = /^([0-9a-fA-F]{64})[ \t]+\*?(\S.*?)\s*$/.exec(line);
    if (!match) throw new Error(`SHA256SUMS.txt line ${index + 1} is not "<sha256>  <name>"`);
    if (sums.has(match[2])) throw new Error(`SHA256SUMS.txt lists ${match[2]} twice`);
    sums.set(match[2], match[1].toLowerCase());
  });
  return sums;
}

export function checkSums(sumsText, bytesByName) {
  let sums;
  try {
    sums = parseSums(sumsText);
  } catch (error) {
    return { checks: [fail("SHA256SUMS.txt is readable", error.message)], warnings: [] };
  }
  const names = [...bytesByName.keys()];
  const unlisted = names.filter((name) => !sums.has(name));
  const extra = [...sums.keys()].filter((name) => !bytesByName.has(name));
  const checks = [
    sameList(sums.keys(), names)
      ? pass("SHA256SUMS.txt lists every other asset, and nothing else")
      : fail(
          "SHA256SUMS.txt lists every other asset, and nothing else",
          [...unlisted.map((n) => `unlisted: ${n}`), ...extra.map((n) => `no such asset: ${n}`)].join("; "),
        ),
  ];
  for (const [name, bytes] of bytesByName) {
    if (sums.has(name)) checks.push(sums.get(name) === sha256(bytes) ? pass(`sha256 ${name}`) : fail(`sha256 ${name}`));
  }
  return { checks, warnings: [] };
}

export function checkUpdaterKeys(previousConfText, currentConfText) {
  try {
    const trusted = parseUpdaterPublicKey(previousConfText);
    const current = parseUpdaterPublicKey(currentConfText);
    const same = trusted.encoded === current.encoded;
    return {
      checks: [same ? pass("the new tag keeps the updater key installed apps trust") : fail("the new tag keeps the updater key installed apps trust")],
      warnings: [],
      publicKey: trusted,
    };
  } catch (error) {
    return { checks: [fail("the updater key is readable at both tags", error.message)], warnings: [], publicKey: null };
  }
}

export function checkSignature(sigName, sigText, targetBytes, version, publicKey) {
  const target = sigName.slice(0, -".sig".length);
  if (targetBytes === undefined) return [fail(`${sigName} signs an asset of this release`, `no ${target}`)];
  let parsed;
  try {
    parsed = parseMinisignSignature(sigText);
  } catch (error) {
    return [fail(`${sigName} is a minisign signature`, error.message)];
  }
  const result = verifyMinisign(parsed, targetBytes, publicKey);
  const named = trustedCommentNames(parsed.trustedComment, target, version);
  return [
    result.madeWithKey ? pass(`${sigName} was made with the updater key`) : fail(`${sigName} was made with the updater key`),
    result.signsFile ? pass(`${sigName} signs ${target}`) : fail(`${sigName} signs ${target}`),
    result.trustedCommentSigned ? pass(`${sigName} trusted comment is signed`) : fail(`${sigName} trusted comment is signed`),
    named ? pass(`${sigName} trusted comment names ${target}`) : fail(`${sigName} trusted comment names ${target}`),
  ];
}

export function checkSignatures(sigTextByName, bytesByName, version, publicKey) {
  const checks = [...sigTextByName].flatMap(([sigName, sigText]) =>
    checkSignature(sigName, sigText, bytesByName.get(sigName.slice(0, -".sig".length)), version, publicKey),
  );
  return { checks, warnings: [] };
}

export function normalizeNotes(text) {
  return String(text ?? "").replace(/\r\n?/g, "\n");
}

export function draftBody(ghOutput) {
  return normalizeNotes(ghOutput).replace(/\n$/, "");
}

export function notesMatch(latestNotes, body) {
  return normalizeNotes(latestNotes) === normalizeNotes(body);
}

export function checkFeed(latestText, previousLatestText, { repo, tag, version, assets, sigTextByName, body, allowChange }) {
  let latest;
  let previousLatest;
  try {
    latest = JSON.parse(latestText);
    previousLatest = JSON.parse(previousLatestText);
  } catch (error) {
    return { checks: [fail("latest.json is readable", error.message)], warnings: [] };
  }
  const checks = [
    latest.version === version ? pass(`latest.json version is ${version}`) : fail(`latest.json version is ${version}`, `found ${latest.version}`),
    Number.isNaN(Date.parse(latest.pub_date)) ? fail("latest.json pub_date is a date") : pass("latest.json pub_date is a date"),
  ];
  const warnings = [];
  const platforms = Object.keys(latest.platforms ?? {});
  const previousPlatforms = Object.keys(previousLatest.platforms ?? {});
  const platformDifferences = [
    ...previousPlatforms.filter((p) => !platforms.includes(p)).map((p) => `missing platform ${p}`),
    ...platforms.filter((p) => !previousPlatforms.includes(p)).map((p) => `unexpected platform ${p}`),
  ];
  if (platformDifferences.length === 0) checks.push(pass(`latest.json platforms match the previous release: ${platforms.sort().join(", ")}`));
  else if (allowChange) warnings.push(...platformDifferences);
  else checks.push(fail("latest.json platforms match the previous release", platformDifferences.join("; ")));

  const prefix = `https://github.com/${repo}/releases/download/${tag}/`;
  for (const [platform, entry] of Object.entries(latest.platforms ?? {})) {
    const url = String(entry?.url ?? "");
    const asset = url.startsWith(prefix) ? url.slice(prefix.length) : null;
    checks.push(asset !== null && assets.includes(asset) ? pass(`${platform} url points at an asset of ${tag}`) : fail(`${platform} url points at an asset of ${tag}`, url));
    const sigText = asset === null ? undefined : sigTextByName.get(`${asset}.sig`);
    if (sigText === undefined) {
      checks.push(fail(`${platform} has a .sig asset`));
    } else {
      const same = String(entry.signature ?? "").trim() === sigText.trim();
      checks.push(same ? pass(`${platform} signature equals ${asset}.sig`) : fail(`${platform} signature equals ${asset}.sig`));
    }
  }
  checks.push(
    notesMatch(latest.notes, body)
      ? pass("latest.json notes match the release body (installed apps show these)")
      : fail(
          "latest.json notes match the release body (installed apps show these)",
          "the draft notes changed after publish-draft ran: re-run that job so latest.json carries them",
        ),
  );
  return { checks, warnings };
}
