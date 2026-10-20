# Security policy

on-n-off reads and changes your coding agents' configuration and can hold their logins, so
security reports are welcome and taken seriously.

## Reporting a vulnerability

Please report vulnerabilities privately through
[GitHub's private vulnerability reporting](https://github.com/giostriquer/on-n-off/security/advisories/new).
Only the maintainer can read the report. Do not open a public issue, pull request or discussion
for a security problem.

Include what you can of:

- the app version and operating system;
- the affected area (for example saved accounts, the updater, a provider's configuration);
- steps to reproduce, or a proof of concept;
- what an attacker gains, and what they need first (local access, a malicious plugin, a network
  position).

Leave out real tokens, account emails and paths from your machine; use placeholders.

on-n-off is maintained by one person. You should get an acknowledgement within 7 days. Once a
fix ships, the advisory is published with credit to you, unless you prefer otherwise.

## Supported versions

Only the [latest release](https://github.com/giostriquer/on-n-off/releases/latest) receives
security fixes. The app updates itself from signed releases, so fixes are not backported.

## Scope

In scope:

- the saved-account vault and the Keychain or Credential Manager items on-n-off writes;
- writes to provider configuration (Claude Code, Codex, Antigravity, Cursor, Muse);
- the commands the window can invoke, and the updater;
- outbound requests, and anything that could send a credential somewhere other than its
  provider.

Out of scope:

- vulnerabilities in the coding agents themselves, the GitHub CLI, or provider services;
- the unsigned Windows installer and the non-notarized macOS app, which are known and
  documented;
- attacks that already require full control of the user's account on the machine.

## Verifying a download

Each release carries GitHub artifact attestations:

```sh
gh attestation verify <downloaded file> --repo giostriquer/on-n-off
```

It also lists SHA-256 checksums in `SHA256SUMS.txt`. Compare a file's checksum
(`shasum -a 256 <file>` on macOS, `Get-FileHash <file>` in PowerShell) with its line there.
Releases after v0.35.6 write that file with LF line endings, so in the download folder
`shasum -a 256 --ignore-missing -c SHA256SUMS.txt` checks every file you downloaded.
