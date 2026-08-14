# on-n-off

A desktop switchboard for Claude and Codex plugins and skills. Global agent config stays the source of truth.

Frontend: React + Tailwind 4 + TanStack Router / Query + lucide-react. Backend: Tauri (Rust).

## Run

```bash
bun install
bun run test
bun run tauri dev
```

Rust adapter tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## Windows release build

```bash
bun run tauri build
```

NSIS/MSI installers are written under `src-tauri/target/release/bundle/`. See [HANDOFF.md](./HANDOFF.md) for coworker smoke-test notes.

Private while the product is taking shape.
