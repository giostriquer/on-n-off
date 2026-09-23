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

test("every job runs on a pinned runner image", { skip }, () => {
  for (const name of ["ci", "bundle", "release", "cache-prune"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      const labels = String(job["runs-on"]).startsWith("${{")
        ? job.strategy.matrix.include.map((entry) => entry.os)
        : [job["runs-on"]];
      for (const label of labels) {
        assert.doesNotMatch(label, /-latest\b/, `${name}.yml ${id} runs on a moving label`);
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

test("macOS jobs resolve Swift packages, with retries, before the build script needs them", { skip }, () => {
  const jobs = [load("ci").jobs.verify, load("bundle").jobs.bundle, load("release").jobs.build];
  const scripts = jobs.map((job) => {
    const resolve = step(job, "Resolve Swift package dependencies");
    assert.equal(resolve.if, "runner.os == 'macOS'");
    assert.ok(resolve.index < firstBuild(job), "resolution must come before the first build");
    return resolve.run;
  });
  assert.match(scripts[0], /\$package = 'src-tauri\/macos\/BrowserBilling'/);
  assert.match(scripts[0], /swift package resolve --package-path \$package/);
  assert.match(scripts[0], /\$attempt -le 3/);
  assert.deepEqual(scripts, [scripts[0], scripts[0], scripts[0]], "keep the three copies identical");
});

test("only pull request runs cancel an in-progress run", { skip }, () => {
  for (const name of ["ci", "bundle"]) {
    assert.equal(load(name).concurrency["cancel-in-progress"], "${{ github.event_name == 'pull_request' }}");
  }
});

test("a failing frontend check does not hide the result of the next one", { skip }, () => {
  const frontend = load("ci").jobs.frontend;
  for (const name of ["Type-check frontend", "Test release verifier"]) {
    assert.match(String(step(frontend, name).if), /!cancelled\(\)/, name);
  }
});

test("a hung test run or release upload ends at its step, not the job timeout", { skip }, () => {
  assert.equal(step(load("ci").jobs.verify, "Test Rust")["timeout-minutes"], 15);
  const upload = step(load("release").jobs["publish-draft"], "Upload and reconcile draft assets");
  assert.ok(upload["timeout-minutes"] <= 10);
  assert.match(upload.run, /WaitForExit\(\d+\)/, "each upload attempt waits a bounded time");
});
