---
name: release-version
description: Cut an on-n-off release end to end — version bump, merge, tag, release approval, draft verification, publish.
disable-model-invocation: true
argument-hint: "[major | minor | patch | X.Y.Z] [PR number]"
---

# Release a version

Ship `main` (or one open PR) as a signed GitHub release that the in-app updater picks up. Every step
ends on its **done** line; move on only when it holds. The repo is `giostriquer/on-n-off`; examples
use `vX.Y.Z` for the new tag and `vP.Q.R` for the previous one.

## 1. Choose the version

- Sync first, in the primary checkout: `git fetch origin && git merge --ff-only origin/main`.
- Previous release: `gh api repos/giostriquer/on-n-off/releases/latest -q .tag_name`.
- `$ARGUMENTS` picks the bump. Without one, read `git log vP.Q.R..origin/main` plus the PR being
  released: any `feat` makes it **minor**, otherwise **patch**. Major only when the user says so.
- What carries the bump: the open PR named in `$ARGUMENTS` (or the one the user just finished), or,
  when everything is already on `main`, a new `release/vX.Y.Z` branch in
  `.worktrees/release-vX.Y.Z` holding only the bump.

**Done:** `vX.Y.Z` is decided and does not exist yet (`git ls-remote --tags origin vX.Y.Z` is empty).

## 2. Bump the version

The release contract (`scripts/check-release-version.ps1`, run by the Release workflow's `validate`
job) requires the same version in all five places:

| File | Field |
| --- | --- |
| `package.json` | `"version"` |
| `src-tauri/tauri.conf.json` | `"version"` |
| `src-tauri/Cargo.toml` | `version` under `[package]` |
| `src-tauri/Cargo.lock` | the `name = "on-n-off"` entry's `version` |
| `src-tauri/macos/SideNotch/Info.plist` | `CFBundleShortVersionString` and `CFBundleVersion` |

Check with `cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps --locked`,
which fails if `Cargo.lock` disagrees. `bun.lock` carries no root version. Commit as
`release: bump on-n-off to vX.Y.Z`, with explicit pathspecs. Title the PR
`release: on-n-off vX.Y.Z with <what ships>`, and add one Summary bullet listing the five bumped files.

**Done:** the bump commit is pushed, and CI's three required checks (`frontend`, `verify-windows`,
`verify-macos`) are green **at that commit**. Dispatch a `workbench:ci-watcher` pinned to the SHA;
on red, use `fix-ci`. The branch must be current with `main` (strict ruleset); merge `main` in and
never rebase.

## 3. Merge

`gh pr merge <n> --repo giostriquer/on-n-off --squash --match-head-commit <bump sha>`

Auto mode refuses this command from the session. Hand the user the exact line to run as
`! gh pr merge …`; a normal squash merge needs no admin bypass. Then fast-forward the primary checkout.
GitHub deletes the merged head branch itself.

**Done:** `gh pr view <n> --json state,mergeCommit` says `MERGED`, and local `main` is at that merge commit.

## 4. Tag

```sh
git tag -a vX.Y.Z <merge sha> -m "on-n-off vX.Y.Z"
git push origin vX.Y.Z
```

Annotated, on the merge commit, with the message format every earlier tag uses. The push starts the
Release workflow. Its `validate` job rejects a tag that is not on `main` or that disagrees with the
five files.

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
node scripts/verify-release.mjs vX.Y.Z vP.Q.R
```

It downloads both releases into `.tmp/release-check/` and checks:

- the asset set is the previous release's, renamed;
- `SHA256SUMS.txt`;
- every minisign `.sig` against the updater key in `tauri.conf.json`;
- `latest.json`: version, platforms, URLs, and signatures identical to the `.sig` files;
- `gh attestation verify` for every installer.

The repo is public, so also read the draft notes (`gh release view vX.Y.Z --json body`). Look for
email addresses, local paths, and names the placeholder rules keep out.

**Done:** the script ends with `vX.Y.Z verified against vP.Q.R.` and the notes are clean. Any
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
