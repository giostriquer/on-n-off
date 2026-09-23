// Run with `bun test scripts/` (CI's frontend job). It parses the workflows with Bun's built-in
// YAML parser, so under `node --test` it skips. Each test pins a choice the workflows make on
// purpose; change the test in the same commit when a choice changes deliberately.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const skip = globalThis.Bun?.YAML ? false : "needs Bun's YAML parser";
const directory = join(dirname(fileURLToPath(import.meta.url)), "..", ".github", "workflows");
const load = (name) => globalThis.Bun.YAML.parse(readFileSync(join(directory, `${name}.yml`), "utf8"));

function step(job, name) {
  const index = job.steps.findIndex((candidate) => candidate.name === name);
  assert.notEqual(index, -1, `no step named "${name}"`);
  return { index, ...job.steps[index] };
}

/** The first step that compiles the Rust crate, and with it the Swift helpers its build script builds. */
function firstBuild(job) {
  const index = job.steps.findIndex((candidate) =>
    /cargo (clippy|test|build)|tauri build|build-bundle\.ps1/.test(candidate.run ?? ""));
  assert.notEqual(index, -1, "no step builds the crate");
  return index;
}

// The one list of runner images, bumped deliberately like rust-toolchain.toml. A bump changes this
// list and every workflow in the same pull request: release's build job restores the cache that
// Bundle saves, and rust-cache's key does not include the image, so those two must never differ.
const pinnedImages = ["ubuntu-24.04", "windows-2025-vs2026", "macos-26"];

test("every job runs on one of the pinned runner images", { skip }, () => {
  for (const name of ["ci", "bundle", "release", "cache-prune"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      const labels = String(job["runs-on"]).startsWith("${{")
        ? job.strategy.matrix.include.map((entry) => entry.os)
        : [job["runs-on"]];
      for (const label of labels) {
        assert.ok(pinnedImages.includes(label), `${name}.yml ${id} runs on ${label}, not a pinned image`);
      }
    }
  }
});

test("ci, bundle and release share one env block, which rust-cache hashes into its key", { skip }, () => {
  const env = load("ci").env;
  assert.deepEqual(load("bundle").env, env);
  assert.deepEqual(load("release").env, env);
});

test("rustfmt fails a verify leg before the cache restore and the slower setup", { skip }, () => {
  const verify = load("ci").jobs.verify;
  const format = step(verify, "Check Rust formatting");
  assert.equal(format.index, step(verify, "Drop unpinned toolchains").index + 1);
  assert.ok(format.index < step(verify, "Cache Rust dependencies").index);
});

test("macOS jobs resolve Swift packages before the build script needs them", { skip }, () => {
  for (const job of [load("ci").jobs.verify, load("bundle").jobs.bundle, load("release").jobs.build]) {
    const resolve = step(job, "Resolve Swift package dependencies");
    assert.equal(resolve.if, "runner.os == 'macOS'");
    assert.equal(resolve.run.trim(), "./scripts/resolve-swift-packages.ps1 -PackagePath src-tauri/macos/BrowserBilling");
    assert.ok(resolve.index < firstBuild(job), "resolution must come before the first build");
  }
  const tests = load("ci").jobs.verify.steps.map((candidate) => candidate.run?.trim());
  assert.ok(tests.includes("./scripts/resolve-swift-packages.test.ps1"), "the verify legs run the resolver's tests");
});

test("only pull request runs cancel an in-progress run", { skip }, () => {
  assert.equal(load("ci").concurrency["cancel-in-progress"], "${{ github.event_name == 'pull_request' }}");
  for (const name of ["bundle", "release", "cache-prune"]) {
    assert.equal(load(name).concurrency["cancel-in-progress"], false, `${name}.yml has no pull request run to cancel`);
  }
});

test("a failing frontend check does not hide the result of the next one", { skip }, () => {
  const frontend = load("ci").jobs.frontend;
  for (const name of ["Type-check frontend", "Test release scripts and workflow contracts"]) {
    assert.match(String(step(frontend, name).if), /!cancelled\(\)/, name);
  }
});

test("a hung test run or release upload ends at its step, not the job timeout", { skip }, () => {
  assert.equal(step(load("ci").jobs.verify, "Test Rust")["timeout-minutes"], 15);
  const upload = step(load("release").jobs["publish-draft"], "Upload and reconcile draft assets");
  assert.ok(upload["timeout-minutes"] <= 10);
  assert.match(upload.run, /WaitForExit\(\d+\)/, "each upload attempt waits a bounded time");
});
