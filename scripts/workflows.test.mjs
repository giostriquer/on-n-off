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

// The one runner image per OS, bumped deliberately like rust-toolchain.toml. A bump replaces an
// entry here and changes every workflow in the same pull request. Keyed by OS so that a staged bump
// cannot list a second image for an OS and leave some workflows on the old one.
const pinnedImages = { ubuntu: "ubuntu-24.04", windows: "windows-2025-vs2026", macos: "macos-26" };

/** The runner label of every one of the job's legs, with its matrix platform where it has a matrix. */
function runnerLegs(job) {
  return String(job["runs-on"]).startsWith("${{")
    ? job.strategy.matrix.include.map((entry) => ({ platform: entry.platform, label: entry.os }))
    : [{ platform: "", label: job["runs-on"] }];
}

test("every job runs on its OS's pinned runner image", { skip }, () => {
  for (const name of ["ci", "bundle", "release", "cache-prune"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      for (const { label } of runnerLegs(job)) {
        const os = String(label).split("-")[0];
        assert.ok(Object.hasOwn(pinnedImages, os), `${name}.yml ${id} runs on ${label}, an OS with no pinned image`);
        assert.equal(label, pinnedImages[os], `${name}.yml ${id} runs on ${label}, not the pinned ${os} image`);
      }
    }
  }
});

// rust-cache's key does not include the runner image, so a cache saved on one image restores on
// another. Release's build job restores the cache that Bundle saves, so the two must build each
// platform on the same image, or a release would reuse a target directory built with another
// image's SDK and linker.
test("release builds each platform on the image of the Bundle cache it restores", { skip }, () => {
  const bundle = load("bundle").jobs.bundle;
  const release = load("release").jobs.build;
  const sharedKey = (job) => step(job, "Cache Rust dependencies").with["shared-key"];
  assert.equal(sharedKey(release), sharedKey(bundle));
  // Every leg, not one per platform: two legs can share a platform, and so a cache.
  const legs = (job) => runnerLegs(job).map(({ platform, label }) => `${platform}:${label}`).sort();
  assert.deepEqual(legs(release), legs(bundle));
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

// The Windows runner image turns off Defender's real-time protection and excludes C:\ and D:\ in its
// own build (actions/runner-images, Configure-WindowsDefender.ps1). An exclusion step here only
// costs its 5 s of cmdlet start-up on every Windows job.
test("no workflow spends a step on Defender exclusions the runner image already has", { skip }, () => {
  for (const name of ["ci", "bundle", "release"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      for (const candidate of job.steps ?? []) {
        assert.doesNotMatch(candidate.run ?? "", /MpPreference/, `${name}.yml ${id}: ${candidate.name}`);
      }
    }
  }
});

// The macOS jobs cache the Swift packages' .build directories: SwiftPM's resolved checkout and the
// SDK modules and objects a cold build spends most of a minute on.
const swiftJobs = () => [
  { name: "ci", job: load("ci").jobs.verify, configuration: "debug" },
  { name: "bundle", job: load("bundle").jobs.bundle, configuration: "release" },
  { name: "release", job: load("release").jobs.build, configuration: "release" },
];
const swiftBuildDirectories = ["src-tauri/macos/SideNotch/.build", "src-tauri/macos/BrowserBilling/.build"];
const lines = (text) => String(text).trim().split("\n").map((line) => line.trim());
/** The key's dash-separated fields, with each `${{ }}` expression standing in as one field. */
const keyFields = (key) => key.replace(/\$\{\{.*?\}\}/g, "EXPR").split("-");

test("macOS jobs restore the Swift build cache before resolving, and stamp sources before building", { skip }, () => {
  for (const { name, job, configuration } of swiftJobs()) {
    const toolchain = step(job, "Identify the Swift toolchain");
    const restore = step(job, "Restore Swift build cache");
    const stamp = step(job, "Stamp Swift sources with their content");
    for (const candidate of [toolchain, restore, stamp]) assert.equal(candidate.if, "runner.os == 'macOS'", `${name}: ${candidate.name}`);
    // The selected Xcode fixes both the compiler and the SDK, and its version file is read without
    // starting Swift, which took a step of its own 6 s on a fresh runner.
    assert.match(toolchain.run, /xcode-select --print-path/, name);
    assert.match(toolchain.run, /version\.plist/, name);
    assert.doesNotMatch(toolchain.run, /xcrun/, `${name}: no Swift start-up just to name the toolchain`);
    assert.match(restore.uses, /^actions\/cache\/restore@[0-9a-f]{40}$/, `${name}: pinned by commit`);
    assert.deepEqual(lines(restore.with.path), swiftBuildDirectories, name);
    // The toolchain and every input the build scripts and native checks read.
    const key = restore.with.key;
    assert.ok(key.startsWith(`v0-swiftpm-${configuration}-\${{ runner.os }}-\${{ runner.arch }}-\${{ steps.swift.outputs.toolchain }}-`), `${name}: ${key}`);
    for (const input of ["Package.swift", "Package.resolved", "Info.plist", "Sources/**", "Tests/**"]) {
      assert.ok(key.includes(`'src-tauri/macos/*/${input}'`), `${name}: the key hashes ${input}`);
    }
    // A source change restores the previous generation and rebuilds only what changed.
    assert.ok(key.endsWith("}}"), name);
    assert.equal(restore.with["restore-keys"].trim(), key.slice(0, key.lastIndexOf("${{")), name);
    assert.ok(toolchain.index < restore.index, name);
    assert.ok(restore.index < step(job, "Resolve Swift package dependencies").index, `${name}: the resolver finds the restored checkout`);
    assert.ok(stamp.index > restore.index, name);
    assert.ok(stamp.index < firstBuild(job), `${name}: stamped before the build script's first swift build`);
    assert.equal(stamp.run.trim(), "bun scripts/stamp-source-times.mjs src-tauri/macos");
    assert.ok(stamp.index > step(job, "Set up Bun").index, name);
  }
});

// cache-prune groups keys by their first five fields and keeps the newest generations of each.
test("a Swift cache key is five identifying fields and two generation hashes, like rust-cache's", { skip }, () => {
  for (const { name, job, configuration } of swiftJobs()) {
    const fields = keyFields(step(job, "Restore Swift build cache").with.key);
    assert.deepEqual(fields.slice(0, 5), ["v0", "swiftpm", configuration, "EXPR", "EXPR"], name);
    assert.equal(fields.length, 7, name);
  }
  const prune = step(load("cache-prune").jobs.prune, "Delete superseded cache generations");
  for (const family of ["v0-rust-*", "v0-swiftpm-*"]) assert.ok(prune.run.includes(`'${family}'`), family);
  assert.match(prune.run, /\(\$_\.key -split '-'\)\[0\.\.4\]/);
});

test("release restores the Swift build cache that Bundle saves", { skip }, () => {
  const restore = (workflow, job) => step(load(workflow).jobs[job], "Restore Swift build cache").with;
  assert.deepEqual(restore("release", "build"), restore("bundle", "bundle"));
});

test("only pushes to main save the Swift build cache, and a release never does", { skip }, () => {
  for (const { name, job } of swiftJobs().filter(({ name }) => name !== "release")) {
    const restore = step(job, "Restore Swift build cache");
    const save = step(job, "Save Swift build cache");
    assert.match(save.uses, /^actions\/cache\/save@[0-9a-f]{40}$/, name);
    assert.equal(save.if, "runner.os == 'macOS' && github.ref == 'refs/heads/main' && steps.swift-cache.outputs.cache-hit != 'true'", name);
    assert.equal(restore.id, "swift-cache", name);
    assert.equal(save.with.key, "${{ steps.swift-cache.outputs.cache-primary-key }}", name);
    assert.deepEqual(lines(save.with.path), swiftBuildDirectories, name);
    assert.equal(save.index, job.steps.length - 1, `${name}: saved after every step that builds Swift`);
  }
  for (const candidate of load("release").jobs.build.steps) {
    assert.doesNotMatch(candidate.uses ?? "", /^actions\/cache(\/save)?@/, `release: ${candidate.name}`);
  }
});

// bun hardlinks from its cache into node_modules, and a hardlink cannot cross from C: to the D:
// workspace, so with the default cache it copied every package: 16-20 s against 7 s.
test("every Windows job that installs bun packages keeps bun's install cache on the workspace drive", { skip }, () => {
  for (const [name, job] of [["ci", load("ci").jobs.verify], ["bundle", load("bundle").jobs.bundle], ["release", load("release").jobs.build]]) {
    const cache = step(job, "Keep the bun cache on the workspace drive");
    assert.equal(cache.if, "runner.os == 'Windows'", name);
    assert.match(cache.run, /BUN_INSTALL_CACHE_DIR=\$\(Join-Path \$env:RUNNER_TEMP /, name);
    assert.match(cache.run, />> \$env:GITHUB_ENV/, name);
    assert.ok(cache.index < step(job, "Install frontend dependencies").index, name);
  }
  // And no Windows job added later installs without it.
  for (const name of ["ci", "bundle", "release"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      const onWindows = runnerLegs(job).some(({ label }) => String(label).startsWith("windows"));
      if (!onWindows || !(job.steps ?? []).some((candidate) => /bun install/.test(candidate.run ?? ""))) continue;
      assert.ok(job.steps.some((candidate) => candidate.name === "Keep the bun cache on the workspace drive"), `${name}.yml ${id}`);
    }
  }
});

// rust-cache deletes the registry sources it restored, so the first cargo command re-extracts them.
// The Windows leg does that in the background, after the restore it extracts from and behind the
// frontend steps, and waits for it before the first build so its output and any hang stay in their
// own step. A failed fetch is a warning: clippy fetches what it needs itself and fails on its own.
test("the Windows verify leg unpacks Rust dependencies in the background before the first build", { skip }, () => {
  const verify = load("ci").jobs.verify;
  const start = step(verify, "Start fetching Rust dependencies");
  const finish = step(verify, "Finish fetching Rust dependencies");
  for (const candidate of [start, finish]) assert.equal(candidate.if, "runner.os == 'Windows'", candidate.name);
  assert.match(start.run, /Start-Process pwsh/);
  // host-tuple limits the fetch to the host's dependencies, the set clippy compiles.
  assert.match(start.run, /cargo fetch --manifest-path src-tauri\/Cargo\.toml --locked --target host-tuple /);
  assert.doesNotMatch(start.run, /rustc -vV/, "cargo resolves the host itself");
  assert.ok(start.index > step(verify, "Cache Rust dependencies").index, "the fetch needs the restored registry");
  assert.ok(start.index < step(verify, "Install frontend dependencies").index, "the fetch overlaps the frontend steps");
  assert.ok(finish.index > step(verify, "Build frontend").index, "nothing is left to overlap after the frontend build");
  assert.ok(finish.index < firstBuild(verify), "the fetch finishes before the first build");
  assert.match(start.run, /finally \{/, "the fetch reports its result however its script ends");
  assert.match(finish.run, /WaitForExit\(\d+\)/, "the wait is bounded");
  for (const candidate of [start, finish]) {
    assert.match(candidate.run, /StartTime\.Ticks/, `${candidate.name}: a reused process id is never waited on or ended`);
  }
  assert.match(finish.run, /exit 0\s*$/, "a failed fetch never fails the job");
});
