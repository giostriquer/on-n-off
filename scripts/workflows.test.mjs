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

// bun hardlinks from its cache into node_modules, and a hardlink cannot cross from C: to the D:
// workspace, so with the default cache it copied every package: 16-20 s against 7 s.
test("the Windows verify leg keeps bun's install cache on the workspace drive", { skip }, () => {
  const verify = load("ci").jobs.verify;
  const cache = step(verify, "Keep the bun cache on the workspace drive");
  assert.equal(cache.if, "runner.os == 'Windows'");
  assert.match(cache.run, /BUN_INSTALL_CACHE_DIR=\$\(Join-Path \$env:RUNNER_TEMP /);
  assert.match(cache.run, />> \$env:GITHUB_ENV/);
  assert.ok(cache.index < step(verify, "Install frontend dependencies").index);
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
  assert.match(start.run, /cargo fetch --manifest-path src-tauri\/Cargo\.toml --locked --target \$Target/);
  assert.ok(start.index > step(verify, "Cache Rust dependencies").index, "the fetch needs the restored registry");
  assert.ok(start.index < step(verify, "Install frontend dependencies").index, "the fetch overlaps the frontend steps");
  assert.ok(finish.index > step(verify, "Build frontend").index, "nothing is left to overlap after the frontend build");
  assert.ok(finish.index < firstBuild(verify), "the fetch finishes before the first build");
  assert.match(finish.run, /exit 0\s*$/, "a failed fetch never fails the job");
});
