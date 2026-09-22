---
name: release-version
description: Cut an on-n-off release end to end — version bump, merge, tag, release approval, draft verification, publish.
disable-model-invocation: true
argument-hint: "[major | minor | patch | X.Y.Z] [PR number]"
---

# Release a version

Ship `main` (or one open PR) as a signed GitHub release that the in-app updater picks up. Every step
ends on its **done** line; move on only when it holds.

- The repo is `giostriquer/on-n-off`.
- Examples use `vX.Y.Z` for the new tag and `vP.Q.R` for the previous one.
- The snippets are POSIX shell: bash or zsh, or Git Bash on Windows.

## 1. Choose the version

- Sync first, in the primary checkout: `git fetch origin --tags && git merge --ff-only origin/main`.
- Previous release: `gh api repos/giostriquer/on-n-off/releases/latest -q .tag_name`.
- `$ARGUMENTS` picks the bump. Without one, read `git log vP.Q.R..origin/main` plus the PR being
  released: any `feat` makes it **minor**, otherwise **patch**. Major only when the user says so.
- What carries the bump: the open PR named in `$ARGUMENTS` (or the one the user just finished), or,
  when everything is already on `main`, a new `release/vX.Y.Z` branch in
  `.worktrees/release-vX.Y.Z` holding only the bump.

**Done:** `vX.Y.Z` is decided and does not exist yet (`git ls-remote --tags origin vX.Y.Z` is empty).

## 2. Bump the version

Set the new version in all six places:

| File | Field | Checked by `validate` |
| --- | --- | --- |
| `package.json` | `"version"` | yes |
| `src-tauri/tauri.conf.json` | `"version"` | yes |
| `src-tauri/Cargo.toml` | `version` under `[package]` | yes |
| `src-tauri/macos/SideNotch/Info.plist` | `CFBundleShortVersionString` | yes |
| `src-tauri/macos/SideNotch/Info.plist` | `CFBundleVersion` | **no** |
| `src-tauri/Cargo.lock` | the `name = "on-n-off"` entry's `version` | **no** |

The Release workflow's `validate` job runs `scripts/check-release-version.ps1`, which checks four of
the six. The last two are on whoever does the bump; a stale `Cargo.lock` gets rewritten by the next
`cargo` command in every worktree and lands in some unrelated PR.

Two checks:

- `cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --locked > /dev/null` fails
  on a stale lock. Keep it without `--no-deps`: that flag skips resolution, so `--locked` never fires.
- `grep -A1 CFBundleVersion src-tauri/macos/SideNotch/Info.plist` shows the new version.

`bun.lock` carries no root version.

Commit as `release: bump on-n-off to vX.Y.Z` with explicit pathspecs. Title the PR
`release: on-n-off vX.Y.Z with <what ships>`, and add one Summary bullet listing the bumped files.

**Done:**

- The bump commit is pushed.
- CI's three required checks (`frontend`, `verify-windows`, `verify-macos`) are green at the PR's
  head. Dispatch a `workbench:ci-watcher` pinned to that SHA; on red, use `fix-ci`.
- The branch is current with `main` (the ruleset is strict). Merge `main` in, never rebase, and let
  CI pass again on the new head.

## 3. Merge

```sh
gh pr merge <n> --repo giostriquer/on-n-off --squash --match-head-commit <PR head SHA that CI passed on>
```

Auto mode refuses this command from the session. Hand the user the exact line to run as
`! gh pr merge …`; a normal squash merge needs no admin bypass. GitHub deletes the merged head
branch itself. Then fast-forward the primary checkout.

**Done:** `gh pr view <n> --json state,mergeCommit` says `MERGED`, and local `main` is at that merge commit.

## 4. Tag

```sh
git tag -a vX.Y.Z <merge sha> -m "on-n-off vX.Y.Z"
git push origin vX.Y.Z
```

- The tag is annotated and sits on the merge commit. The message follows recent tags; older ones
  added a summary after the version.
- The `release-tags` ruleset restricts creating `refs/tags/v*` to accounts with its bypass. A
  refusal means the pushing account lacks it, so hand the push to the user.
- The push starts the Release workflow. Its `validate` job rejects a tag that is not on `main` or
  that disagrees with the four checked sources.

**Done:** the run exists: `gh run list --workflow release.yml --json databaseId,headBranch`, with
`headBranch` equal to the tag. If `validate` fails, stop and report: a pushed tag moves only with
the user's say-so.

## 5. Approve the release environment

`build-nsis` and `build-dmg` wait on the `release` environment, which holds the updater signing
secrets. Once the run's status is `waiting` (validate passed):

```sh
gh api repos/giostriquer/on-n-off/actions/runs/<run>/pending_deployments -q '.[].environment.id'
printf '{"environment_ids":[<id>],"state":"approved","comment":"Release vX.Y.Z"}' \
  | gh api -X POST repos/giostriquer/on-n-off/actions/runs/<run>/pending_deployments --input -
```

**Done:** the run concludes `success`, with `validate`, `build-nsis`, `build-dmg` and `publish-draft`
all green, and `gh release view vX.Y.Z` shows a draft. Builds take about 10 minutes; wait with a
background `until` loop on the run status, not repeated polling.

## 6. Verify the draft

```sh
bun scripts/verify-release.mjs vX.Y.Z vP.Q.R
```

It downloads the draft into `.tmp/release-check/`, and for the previous release only its asset
names and `latest.json`. Then it checks:

- the asset set is the previous release's, renamed;
- `SHA256SUMS.txt`;
- the updater key in `tauri.conf.json` at `vX.Y.Z` equals the one at `vP.Q.R`, which is the key
  installed apps trust (needs both tags locally: `git fetch --tags`);
- every minisign `.sig`, under that key, for the file it names;
- `latest.json`: version, platforms, URLs, signatures identical to the `.sig` files, and notes
  identical to the draft's body;
- `gh attestation verify` for every installer, pinned to `release.yml` running at `refs/tags/vX.Y.Z`
  on a GitHub-hosted runner, so an older release's build cannot pass as this one.

Then read the notes for leaks: email addresses, local paths, and names the placeholder rules keep
out. Read them in `latest.json` (`jq -r .notes .tmp/release-check/vX.Y.Z/latest.json`), because
that copy is what installed apps show in their update prompt. The release page shows the draft body.

Fixing a leak, or any other notes change:

1. Edit the draft: `gh release edit vX.Y.Z --notes-file <fixed.md>`.
2. Re-run the `publish-draft` job so `latest.json` is rebuilt from the edited body:
   `gh run rerun <run> --job <id>`, where `<id>` is the job's `databaseId` from
   `gh run view <run> --json jobs -q '.jobs[] | {name, databaseId}'`, not the number in its URL.
3. Run the verifier again.

The build artifacts that job re-uses are kept for one day. After that, cut the next patch version
instead.

An intentional change to the asset set or platforms FAILs the first comparison against the previous
release. To accept one:

1. Confirm the new names against `release.yml`, whose `$expectedNames` list and nine-asset count
   are authoritative.
2. Re-run with `--allow-asset-change`.

**Done:** the script ends with `vX.Y.Z verified against vP.Q.R.` and the notes are clean. Any other
`FAIL` stops the release: report it and leave the draft unpublished.

## 7. Publish and confirm

```sh
gh release edit vX.Y.Z --repo giostriquer/on-n-off --draft=false --latest
```

**Done:** all three of these hold.

- `gh api repos/giostriquer/on-n-off/releases/latest -q .tag_name` prints `vX.Y.Z`. Use this, not
  `gh release view`, which has no latest flag.
- The live updater feed matches the verified file byte for byte:
  `curl -fsSL https://github.com/giostriquer/on-n-off/releases/latest/download/latest.json`
  compared with `cmp` against `.tmp/release-check/vX.Y.Z/latest.json`. Retry briefly if the CDN
  still serves the old one.
- The released PR's worktree and local branch are removed.

## Report

Give the user:

- the release URL, the tag, and the merge commit;
- every check group's result;
- the steps they ran by hand;
- anything left undone.

A published release is not rewritten. Fix a bad one by releasing the next patch version.
