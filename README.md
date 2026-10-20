# on-n-off

Manage your **Claude Code, Codex, Antigravity, Cursor, and Muse** setup from one desktop
app for Windows and macOS.

[Download](https://github.com/giostriquer/on-n-off/releases/latest) ·
[Report a bug](https://github.com/giostriquer/on-n-off/issues) ·
[Documentation](docs/architecture/README.md)

## Features

- **Plugins, skills, and MCP servers** — see what's installed and enable or disable items where supported.
- **Accounts** — save Claude, Codex and Muse logins, switch accounts, and optionally remember new sign-ins automatically.
- **Usage and limits** — view usage estimates, subscription limits, and reset times for Claude, Codex, and Muse.
- **Pull requests** — follow your GitHub reviews, CI checks, and merge status.
- **Side notch** — keep usage and pull requests visible at the edge of your screen.
- **Project scope** — inspect agent configuration globally or for a specific project.

Support varies by provider; Cursor currently supports browsing installed items.
Codex account switching requires its running clients to be closed; Claude Code can stay open.
Codex cards also show how long the subscription is paid for and whether it renews.

## Install

Get the latest build from [GitHub Releases](https://github.com/giostriquer/on-n-off/releases/latest).

| Platform | Download |
| --- | --- |
| Windows x64 | `.exe` installer |
| macOS Apple Silicon | `.dmg` disk image |

The side notch requires Windows 11 on Windows. If your operating system blocks
the first launch, see the [Windows](HANDOFF.md#what-you-get) or
[macOS](HANDOFF.md#macos) installation notes.

on-n-off is in early development. It works with your existing agent installations
and can change their configuration.

## Get started

1. Install and sign in to the coding agents you use.
2. Open on-n-off and select a provider to browse its installed items.
3. Open **Limits** to view usage and add saved accounts. Enable automatic account saving if you want new sign-ins remembered.

For pull requests, install the [GitHub CLI](https://cli.github.com/) and sign in
with `gh auth login`. Appearance, notifications, and the side notch are in **Settings**.

## Development

Install [Bun](https://bun.sh/), [Rust](https://rustup.rs/), and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

```sh
git clone https://github.com/giostriquer/on-n-off.git
cd on-n-off
bun install
bun run tauri dev
```

Built with Tauri, Rust, React, and TypeScript.

- [Architecture](docs/architecture/README.md)
- [Provider support](PROVIDERS.md)
- [Platform and packaging notes](OS.md)
- [Contributing](CONTRIBUTING.md)
- [Manual testing](HANDOFF.md)

## Privacy

on-n-off has no telemetry, analytics or crash reporting, and no server of its own. Your
configuration, transcripts and saved logins stay on your machine. The app itself connects only to:

- `chatgpt.com` and `auth.openai.com`, for Codex limits and subscription details and to renew saved
  Codex logins;
- `api.meta.ai`, for Muse limits;
- `api.github.com`, for pull requests and to resolve skills and agents installed from GitHub;
- `codeload.github.com`, to download those installs;
- `raw.githubusercontent.com`, for plugin details and LiteLLM's public model price list behind
  usage estimates;
- `github.com`, for the update feed on GitHub Releases.

Claude limits come from Claude Code's own usage report: on-n-off sends nothing to Anthropic itself.
The tools it starts (`claude`, `codex`, `agy`, `muse`, `gh` and `npx`) make their own requests as
usual, including installs from sources you enter.

## Contributing

Bug reports and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md) and the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately, as described in
[SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). Usage analytics include work derived from
[T3 Code](https://github.com/pingdotgg/t3code). See
[third-party notices](THIRD_PARTY_NOTICES.md) for credits and license details.

on-n-off is an independent project, not affiliated with, endorsed by or sponsored by Anthropic,
OpenAI, Google, Anysphere or Meta. Claude Code, Codex, Antigravity, Cursor and Muse are trademarks
of their respective owners.
