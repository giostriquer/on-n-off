// Run with `bun test scripts/` (CI's frontend job). It parses the workflows with Bun's built-in
// YAML parser, so under `node --test` it skips. Each test pins a choice the workflows make on
// purpose; change the test in the same commit when a choice changes deliberately.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
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

// The native legs are verify.yml, which ci.yml calls once per OS with that OS's runner image.
const verifyWorkflow = "./.github/workflows/verify.yml";
const verifyCalls = () => Object.entries(load("ci").jobs).filter(([, job]) => job.uses === verifyWorkflow);
const lint = () => load("verify").jobs.lint;
const testJob = () => load("verify").jobs.test;

/** The runner label of every one of the job's legs, with its platform where it has more than one. */
function runnerLegs(job) {
  if (job.uses === verifyWorkflow) return [{ platform: job.with.platform, label: job.with.os }];
  if (job["runs-on"] === "${{ inputs.os }}") return verifyCalls().flatMap(([, call]) => runnerLegs(call));
  return String(job["runs-on"]).startsWith("${{")
    ? job.strategy.matrix.include.map((entry) => ({ platform: entry.platform, label: entry.os }))
    : [{ platform: "", label: job["runs-on"] }];
}

test("every job runs on its OS's pinned runner image", { skip }, () => {
  for (const name of ["ci", "verify", "bundle", "release", "cache-prune"]) {
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

// A caller's env block never reaches a called workflow, so CI's cargo settings live in verify.yml.
test("verify, bundle and release share one env block, which rust-cache hashes into its key", { skip }, () => {
  const env = load("verify").env;
  assert.deepEqual(env, { CARGO_INCREMENTAL: 0, CARGO_PROFILE_DEV_DEBUG: 0, CARGO_TERM_COLOR: "always" });
  assert.deepEqual(load("bundle").env, env);
  assert.deepEqual(load("release").env, env);
  assert.equal(load("ci").env, undefined, "ci.yml's env would not reach the verify jobs");
});

test("rustfmt fails the lint job before the cache restore and the slower setup", { skip }, () => {
  const format = step(lint(), "Check Rust formatting");
  assert.equal(format.index, step(lint(), "Drop unpinned toolchains").index + 1);
  assert.ok(format.index < step(lint(), "Cache Rust dependencies").index);
});

test("macOS jobs resolve Swift packages before the build script needs them", { skip }, () => {
  for (const job of [lint(), testJob(), load("bundle").jobs.bundle, load("release").jobs.build]) {
    const resolve = step(job, "Resolve Swift package dependencies");
    assert.equal(resolve.if, "runner.os == 'macOS'");
    assert.equal(resolve.run.trim(), "./scripts/resolve-swift-packages.ps1 -PackagePath src-tauri/macos/BrowserBilling");
    assert.ok(resolve.index < firstBuild(job), "resolution must come before the first build");
  }
  const tests = lint().steps.map((candidate) => candidate.run?.trim());
  assert.ok(tests.includes("./scripts/resolve-swift-packages.test.ps1"), "the lint jobs run the resolver's tests");
});

test("only pull request runs cancel an in-progress run", { skip }, () => {
  assert.equal(load("ci").concurrency["cancel-in-progress"], "${{ github.event_name == 'pull_request' }}");
  // A called workflow's concurrency group matching its caller's cancels the caller.
  assert.equal(load("verify").concurrency, undefined, "verify.yml runs under ci.yml's group");
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
  assert.equal(step(testJob(), "Test Rust")["timeout-minutes"], 15);
  const upload = step(load("release").jobs["publish-draft"], "Upload and reconcile draft assets");
  assert.ok(upload["timeout-minutes"] <= 10);
  assert.match(upload.run, /WaitForExit\(\d+\)/, "each upload attempt waits a bounded time");
});

// The Windows runner image turns off Defender's real-time protection and excludes C:\ and D:\ in its
// own build (actions/runner-images, Configure-WindowsDefender.ps1). An exclusion step here only
// costs its 5 s of cmdlet start-up on every Windows job.
test("no workflow spends a step on Defender exclusions the runner image already has", { skip }, () => {
  for (const name of ["ci", "verify", "bundle", "release"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      for (const candidate of job.steps ?? []) {
        assert.doesNotMatch(candidate.run ?? "", /MpPreference/, `${name}.yml ${id}: ${candidate.name}`);
      }
    }
  }
});

// The macOS jobs cache the Swift packages' .build directories: SwiftPM's resolved checkout and the
// SDK modules and objects a cold build spends most of a minute on. All of them restore through one
// local action, so the toolchain step and the key exist once. One job per key family saves: the
// lint job, whose native checks build a superset of what the test job's build script does, and
// Bundle, whose entry Release restores.
const swiftCacheAction = "./.github/actions/restore-swift-build";
const loadAction = (path) => globalThis.Bun.YAML.parse(readFileSync(join(directory, "..", "..", path, "action.yml"), "utf8"));
const swiftJobs = () => [
  { name: "verify lint", job: lint(), configuration: "debug", saves: true },
  { name: "verify test", job: testJob(), configuration: "debug", saves: false },
  { name: "bundle", job: load("bundle").jobs.bundle, configuration: "release", saves: true },
  { name: "release", job: load("release").jobs.build, configuration: "release", saves: false },
];
const swiftBuildDirectories = ["src-tauri/macos/SideNotch/.build", "src-tauri/macos/BrowserBilling/.build"];
const lines = (text) => String(text).trim().split("\n").map((line) => line.trim());
/** The key's dash-separated fields, with each `${{ }}` expression standing in as one field. */
const keyFields = (key) => key.replace(/\$\{\{.*?\}\}/g, "EXPR").split("-");

test("macOS jobs restore the Swift build cache through one action, before resolving and building", { skip }, () => {
  for (const { name, job, configuration } of swiftJobs()) {
    const restore = step(job, "Restore Swift build cache");
    assert.equal(restore.if, "runner.os == 'macOS'", name);
    assert.equal(restore.uses, swiftCacheAction, name);
    assert.deepEqual(restore.with, { configuration }, name);
    assert.ok(restore.index > step(job, "Set up Bun").index, `${name}: the action stamps sources with bun`);
    assert.ok(restore.index < step(job, "Resolve Swift package dependencies").index, `${name}: the resolver finds the restored checkout`);
    assert.ok(restore.index < firstBuild(job), `${name}: restored and stamped before the build script's first swift build`);
  }
});

test("the Swift cache action keys on the toolchain and the inputs SwiftPM tracks, then stamps sources", { skip }, () => {
  const action = loadAction(swiftCacheAction);
  assert.equal(action.runs.using, "composite");
  const toolchain = step(action.runs, "Identify the Swift toolchain");
  const restore = step(action.runs, "Restore the packages' build directories");
  const stamp = step(action.runs, "Stamp Swift sources with their content");
  // The selected Xcode fixes both the compiler and the SDK, and its version file is read without
  // starting Swift, which took a step of its own 6 s on a fresh runner.
  assert.match(toolchain.run, /xcode-select --print-path/);
  assert.match(toolchain.run, /version\.plist/);
  assert.doesNotMatch(toolchain.run, /xcrun/, "no Swift start-up just to name the toolchain");
  // One key field per configuration, which cache-prune's grouping relies on.
  assert.match(toolchain.run, /-cnotin 'debug', 'release'/);
  assert.match(restore.uses, /^actions\/cache\/restore@[0-9a-f]{40}$/, "pinned by commit");
  assert.deepEqual(lines(restore.with.path), swiftBuildDirectories);
  const key = restore.with.key;
  assert.ok(key.startsWith("v0-swiftpm-${{ inputs.configuration }}-${{ runner.os }}-${{ runner.arch }}-${{ steps.toolchain.outputs.toolchain }}-"), key);
  for (const input of ["Package.swift", "Package.resolved", "Sources/**", "Tests/**"]) {
    assert.ok(key.includes(`'src-tauri/macos/*/${input}'`), `the key hashes ${input}`);
  }
  // Info.plist reaches the notch helper only through a linker flag SwiftPM does not track, so
  // native_build.rs relinks the helper on every build rather than trusting a restored one, and a
  // release's version bump keeps the key Bundle saved before it. The CI leg's native checks fail a
  // helper that embeds another Info.plist than the one beside it.
  assert.doesNotMatch(key, /Info\.plist/);
  const buildScript = readFileSync(join(directory, "..", "..", "src-tauri", "native_build.rs"), "utf8");
  const removal = buildScript.indexOf("fs::remove_file(&legacy);");
  assert.ok(removal !== -1 && removal < buildScript.indexOf('.args(["swift", "build", "--package-path"])'), "the helper is removed before the Swift build");
  assert.match(step(lint(), "Check native notch models and lifecycle").run, /bun scripts\/check-native-notch\.mjs src-tauri\/target\/debug\/on-n-off-notch/);
  // A source change restores the previous generation and rebuilds only what changed.
  assert.ok(key.endsWith("}}"));
  assert.equal(restore.with["restore-keys"].trim(), key.slice(0, key.lastIndexOf("${{")));
  assert.ok(toolchain.index < restore.index);
  assert.ok(restore.index < stamp.index);
  assert.equal(stamp.run.trim(), "bun scripts/stamp-source-times.mjs src-tauri/macos");
  for (const candidate of action.runs.steps.filter((candidate) => candidate.run)) assert.equal(candidate.shell, "pwsh", candidate.name);
  assert.equal(action.outputs["cache-hit"].value, "${{ steps.restore.outputs.cache-hit }}");
  assert.equal(action.outputs["cache-primary-key"].value, "${{ steps.restore.outputs.cache-primary-key }}");
});

// cache-prune groups keys on every field before their two trailing generation hashes and keeps the
// newest generations of each group.
const pruneGroup = (key) => key.split("-").slice(0, -2).join("-");

test("cache-prune groups a key on the fields before its two generation hashes", { skip }, () => {
  const prune = step(load("cache-prune").jobs.prune, "Delete superseded cache generations");
  for (const family of ["v0-rust-*", "v0-swiftpm-*"]) assert.ok(prune.run.includes(`'${family}'`), family);
  assert.match(prune.run, /Group-Object \{ \$fields = \$_\.key -split '-'; \$fields\[0\.\.\(\$fields\.Count - 3\)\] -join '-' \}/);
  // v0-swiftpm-<configuration>-<os>-<arch>, then <toolchain>-<package inputs>.
  const fields = keyFields(step(loadAction(swiftCacheAction).runs, "Restore the packages' build directories").with.key);
  assert.deepEqual(fields, ["v0", "swiftpm", "EXPR", "EXPR", "EXPR", "EXPR", "EXPR"]);
});

// Each job caches exactly what it builds under a key of its own: clippy's metadata and the debug
// application in the lint job, the test profile's dependencies in the test job. A shared key would
// make the two jobs overwrite each other's entry with half of what the other one needs.
test("every job that restores the Rust cache has a key of its own, saved only from main", { skip }, () => {
  const cache = (job) => step(job, "Cache Rust dependencies").with;
  assert.equal(cache(lint())["shared-key"], "verify-lint-${{ inputs.platform }}");
  assert.equal(cache(testJob())["shared-key"], "verify-test-${{ inputs.platform }}");
  for (const job of [lint(), testJob()]) {
    assert.equal(cache(job)["save-if"], "${{ github.ref == 'refs/heads/main' }}");
    assert.equal(cache(job).workspaces, "src-tauri");
  }
  // rust-cache's key is v0-rust-<shared-key>-<os>-<arch>-<environment>-<lockfiles>. Expand every
  // shared key for every leg that uses it and check that no two jobs land in one prune group.
  const rustOs = { windows: "Windows_NT-x64", macos: "Darwin-arm64" };
  const groups = new Map();
  const add = (sharedKey, platform, owner) => {
    const key = `v0-rust-${sharedKey.replace(/\$\{\{ (inputs|matrix)\.platform \}\}/, platform)}-${rustOs[platform]}-15b2903b-f310a12c`;
    const group = pruneGroup(key);
    assert.ok(!groups.has(group), `${owner} and ${groups.get(group)} share the prune group ${group}`);
    groups.set(group, owner);
  };
  for (const [, call] of verifyCalls()) {
    for (const [id, job] of Object.entries(load("verify").jobs)) add(cache(job)["shared-key"], call.with.platform, `verify ${id} ${call.with.platform}`);
  }
  for (const { platform } of runnerLegs(load("bundle").jobs.bundle)) add(cache(load("bundle").jobs.bundle)["shared-key"], platform, `bundle ${platform}`);
  assert.equal(groups.size, 6);
});

test("release restores the Swift build cache that Bundle saves", { skip }, () => {
  const restore = (workflow, job) => step(load(workflow).jobs[job], "Restore Swift build cache");
  assert.equal(restore("release", "build").uses, restore("bundle", "bundle").uses);
  assert.deepEqual(restore("release", "build").with, restore("bundle", "bundle").with);
});

test("only pushes to main save the Swift build cache, one job per key family", { skip }, () => {
  for (const { name, job } of swiftJobs().filter(({ saves }) => saves)) {
    const restore = step(job, "Restore Swift build cache");
    const save = step(job, "Save Swift build cache");
    assert.match(save.uses, /^actions\/cache\/save@[0-9a-f]{40}$/, name);
    assert.equal(save.if, "runner.os == 'macOS' && github.ref == 'refs/heads/main' && steps.swift-cache.outputs.cache-hit != 'true'", name);
    assert.equal(restore.id, "swift-cache", name);
    assert.equal(save.with.key, "${{ steps.swift-cache.outputs.cache-primary-key }}", name);
    assert.deepEqual(lines(save.with.path), swiftBuildDirectories, name);
    assert.equal(save.index, job.steps.length - 1, `${name}: saved after every step that builds Swift`);
  }
  // The action only restores; a plain actions/cache would save in its post step.
  const restoreOnly = swiftJobs().filter(({ saves }) => !saves).flatMap(({ job }) => job.steps);
  for (const candidate of [...restoreOnly, ...loadAction(swiftCacheAction).runs.steps]) {
    assert.doesNotMatch(candidate.uses ?? "", /^actions\/cache(\/save)?@/, candidate.name);
  }
});

// bun hardlinks from its cache into node_modules, and a hardlink cannot cross from C: to the D:
// workspace, so with the default cache it copied every package: 16-20 s against 7 s.
test("every Windows job that installs bun packages keeps bun's install cache on the workspace drive", { skip }, () => {
  for (const [name, job] of [["verify lint", lint()], ["verify test", testJob()], ["bundle", load("bundle").jobs.bundle], ["release", load("release").jobs.build]]) {
    const cache = step(job, "Keep the bun cache on the workspace drive");
    assert.equal(cache.if, "runner.os == 'Windows'", name);
    assert.match(cache.run, /BUN_INSTALL_CACHE_DIR=\$\(Join-Path \$env:RUNNER_TEMP /, name);
    assert.match(cache.run, />> \$env:GITHUB_ENV/, name);
    assert.ok(cache.index < step(job, "Install frontend dependencies").index, name);
  }
  // And no Windows job added later installs without it.
  for (const name of ["ci", "verify", "bundle", "release"]) {
    for (const [id, job] of Object.entries(load(name).jobs)) {
      const onWindows = runnerLegs(job).some(({ label }) => String(label).startsWith("windows"));
      if (!onWindows || !(job.steps ?? []).some((candidate) => /bun install/.test(candidate.run ?? ""))) continue;
      assert.ok(job.steps.some((candidate) => candidate.name === "Keep the bun cache on the workspace drive"), `${name}.yml ${id}`);
    }
  }
});

// rust-cache deletes the registry sources it restored, so the first cargo command re-extracts them.
// Both Windows jobs do that in the background, after the restore they extract from and behind the
// frontend steps, and wait for it before the first build so its output and any hang stay in their
// own step. A failed fetch is a warning: cargo fetches what it needs itself and fails on its own.
test("the Windows verify jobs unpack Rust dependencies in the background before the first build", { skip }, () => {
  for (const verify of [lint(), testJob()]) backgroundFetch(verify);
  const run = (job, name) => step(job, name).run;
  for (const name of ["Start fetching Rust dependencies", "Finish fetching Rust dependencies"]) {
    assert.equal(run(testJob(), name), run(lint(), name), `${name} is one script in both jobs`);
  }
});

function backgroundFetch(verify) {
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
}

// The ruleset requires check runs named exactly frontend, verify-windows and verify-macos. Each OS's
// lint and test jobs run in verify.yml, called once per OS, and a small job per OS answers for them
// under the required name.
const requiredChecks = ["frontend", "verify-windows", "verify-macos"];
const aggregators = { "verify-windows": "windows", "verify-macos": "macos" };

test("both OSes run the lint and the test job, neither of which can be skipped", { skip }, () => {
  const calls = Object.fromEntries(verifyCalls().map(([id, call]) => [id, call]));
  assert.deepEqual(Object.keys(calls).sort(), ["macos", "windows"]);
  for (const [id, call] of Object.entries(calls)) {
    assert.deepEqual(call.with, { platform: id, os: pinnedImages[id] }, id);
    assert.equal(call.if, undefined, `${id}: the call always runs`);
  }
  const jobs = load("verify").jobs;
  assert.deepEqual(Object.keys(jobs).sort(), ["lint", "test"]);
  for (const [id, job] of Object.entries(jobs)) {
    assert.equal(job["runs-on"], "${{ inputs.os }}", id);
    // A called workflow whose job was skipped still reports success to its caller.
    assert.equal(job.if, undefined, `${id} has no condition that could skip it`);
  }
  const runs = (job) => job.steps.map((candidate) => candidate.run ?? "").join("\n");
  assert.match(runs(lint()), /cargo fmt /);
  assert.match(runs(lint()), /cargo clippy /);
  assert.match(runs(lint()), /tauri build --debug --no-bundle/);
  assert.match(runs(lint()), /check-native-notch\.mjs/);
  assert.match(runs(testJob()), /cargo test /);
  assert.doesNotMatch(runs(testJob()), /cargo (fmt|clippy)|tauri build|\.test\.ps1/);
  for (const job of [lint(), testJob()]) assert.equal(job["timeout-minutes"], 45);
});

test("the lint jobs run every PowerShell script test, and the test jobs none", { skip }, () => {
  const scripts = readdirSync(join(directory, "..", "..", "scripts")).filter((file) => file.endsWith(".test.ps1")).sort();
  assert.ok(scripts.length >= 6, scripts.join(", "));
  const runsIn = (job) => job.steps.flatMap((candidate) => candidate.run?.match(/scripts\/[\w-]+\.test\.ps1/g) ?? []).map((path) => path.slice("scripts/".length));
  assert.deepEqual(runsIn(lint()).sort(), scripts);
  assert.deepEqual(runsIn(testJob()), []);
  // Before the first cargo command, while the Windows fetch is still unpacking in the background.
  for (const script of scripts) {
    const index = lint().steps.findIndex((candidate) => candidate.run?.includes(`scripts/${script}`));
    assert.ok(index > step(lint(), "Install frontend dependencies").index && index < firstBuild(lint()), script);
  }
});

test("only the per-OS aggregators and frontend carry a required check's name", { skip }, () => {
  const ci = load("ci").jobs;
  const names = Object.values(ci).map((job) => job.name);
  for (const required of requiredChecks) assert.equal(names.filter((name) => name === required).length, 1, required);
  // A called workflow's check runs are named `<caller name> / <job name>`.
  const calledRuns = verifyCalls().flatMap(([, call]) => Object.values(load("verify").jobs).map((job) => `${call.name} / ${job.name}`));
  assert.deepEqual(calledRuns.sort(), ["macos / lint", "macos / test", "windows / lint", "windows / test"]);
  for (const name of calledRuns) assert.ok(!requiredChecks.includes(name), name);
  assert.equal(ci.frontend.name, "frontend");
});

test("each required verify check needs only its own OS and fails unless every needed job succeeded", { skip }, () => {
  const ci = load("ci").jobs;
  for (const [name, call] of Object.entries(aggregators)) {
    const job = ci[name];
    assert.equal(job.name, name);
    assert.deepEqual([job.needs].flat(), [call], `${name} needs only the ${call} jobs`);
    assert.equal(job.if, "always()", `${name} runs, and so reports, even when a needed job failed or was cancelled`);
    assert.equal(job["runs-on"], pinnedImages.ubuntu);
    assert.equal(job.steps.length, 1);
    const [check] = job.steps;
    assert.equal(check.env.NEEDS, "${{ toJSON(needs) }}");
    assert.match(check.run, /-cne 'success'/, "anything but exactly success fails");
    assert.match(check.run, /exit 1/);
  }
  assert.deepEqual(ci["verify-macos"].steps, ci["verify-windows"].steps);
});

const pwsh = globalThis.Bun?.which?.("pwsh");
test("the aggregator step fails for every result but success, and when it needs nothing", { skip: skip || (pwsh ? false : "needs pwsh") }, () => {
  const script = load("ci").jobs["verify-windows"].steps[0].run;
  const outcome = (needs) => {
    const result = globalThis.Bun.spawnSync([pwsh, "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script], {
      env: { ...process.env, NEEDS: JSON.stringify(needs) },
    });
    return result.exitCode;
  };
  const one = (result) => ({ windows: { result, outputs: {} } });
  assert.equal(outcome(one("success")), 0);
  assert.equal(outcome({ lint: { result: "success" }, test: { result: "success" } }), 0);
  for (const result of ["failure", "cancelled", "skipped", "Success", ""]) assert.notEqual(outcome(one(result)), 0, result);
  assert.notEqual(outcome({ lint: { result: "success" }, test: { result: "skipped" } }), 0);
  assert.notEqual(outcome({}), 0, "a check that needs nothing checks nothing");
});
