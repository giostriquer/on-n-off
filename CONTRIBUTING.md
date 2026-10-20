# Contributing to on-n-off

Thanks for helping. Bug reports, fixes and provider improvements are all welcome. Everyone taking
part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting

- **Bugs and feature ideas:** open an [issue](https://github.com/giostriquer/on-n-off/issues/new/choose).
- **Security problems:** never in a public issue; see [SECURITY.md](SECURITY.md).

For anything bigger than a small fix, open an issue first so we can agree on the approach before
you spend time on it.

## Setting up

Install [Bun](https://bun.sh/), [Rust](https://rustup.rs/) and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.
`rust-toolchain.toml` pins the compiler, and rustup installs it on first use. On macOS the side
notch helper needs full Xcode (see [OS.md](OS.md#native-side-notch-macos)).

```sh
git clone https://github.com/giostriquer/on-n-off.git
cd on-n-off
bun install
bun run tauri dev
```

The app reads and can change your real agent configuration (`~/.claude`, `~/.codex` and the
others). While developing, prefer read-only screens, and use throwaway accounts for anything that
switches or saves logins.

## Where things are

- [Architecture](docs/architecture/README.md): the subsystems and how they connect.
- [CONTEXT.md](CONTEXT.md): the domain names to use in code, docs and pull requests.
- [Development guide](docs/development.md): boundaries, test layout and every check command.
- [PROVIDERS.md](PROVIDERS.md): what each provider supports and where its files live.
- [OS.md](OS.md): platform, packaging and CI notes.

## Making a change

- **Tests first.** Write the regression test, watch it fail for the intended reason, then fix.
  Tests never touch a real home directory: they use the scratch roots described in the
  [development guide](docs/development.md#test-structure).
- **No code comments.** Rationale goes in the guide that owns the subject
  ([why](docs/development.md#code-comments)); CI rejects comment lines in source files.
- **Provider changes** update [PROVIDERS.md](PROVIDERS.md) in the same pull request.
- **Both platforms.** Windows and macOS must both build; CI checks both.
- **Placeholders only.** Fixtures, screenshots, docs and commit messages use invented data
  (`you@example.com`, `/Users/me`, `acme`), never real emails, account ids, paths or tokens.
  Screenshots show mock data (`bun run ui:shots` renders the UI with it).

Before you push, run the checks for what you touched, as listed in
[Commands](docs/development.md#commands).

## Pull requests

- One change per pull request, from a branch named `<type>/<short-slug>`.
- Title it like the history: `type(scope): what changed`, where type is one of `feat`, `fix`,
  `perf`, `refactor`, `docs`, `test` or `chore`.
- Fill in the template: what changed, why, and how you verified it. Include before and after
  captures for visual changes.
- The required CI checks must pass. Pull requests are squash-merged.

## License

By contributing, you agree that your contributions are licensed under the project's
[MIT License](LICENSE).
