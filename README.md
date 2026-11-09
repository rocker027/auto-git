# note-sync

Lightweight macOS menu-bar tool for Markdown note folders:

- Watch multiple folders and periodically `git add` + `git commit` (`update N files`)
- `git init` when a folder is not a repo
- Search notes via external [`qmd`](https://github.com/tobi/qmd) (`qmd search`)
- Open one file at a time for read/edit; unsaved edits pause auto-commit

## Requirements

- Rust toolchain
- `git` on PATH
- `qmd` on PATH (optional for search)

## Run

```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
cargo run --release
```

### macOS `.app`

```bash
./scripts/package_app.sh
open "$HOME/Applications/Note Sync.app"
```

打包會用 `assets/icon/`（Codex 設計）產生 `.icns`，輸出到 `dist/Note Sync.app`，並可複製到 `~/Applications`。

Config: `~/Library/Application Support/app.NoteSync.note-sync/config.json`（via `directories` crate）。

## Tests

```bash
cargo test
```

## SDLC

`AISDLC-2026-1109-note-sync-mvp` in `ai-sdlc-hub` project `auto-git`.
