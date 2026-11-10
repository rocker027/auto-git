# note-sync

Lightweight macOS menu-bar tool for Markdown note folders:

- Watch multiple folders and periodically `git add` + `git commit` (`update N files`)
- `git init` when a folder is not a repo
- Left: every Markdown note in the watched folders, newest first; the search box filters by name and text as you type
- Optional: with [`qmd`](https://github.com/tobi/qmd) on PATH, matching notes from other qmd collections appear under 「其他地方」
- Right: one note open for editing; ⌘S saves. Unsaved edits pause auto-commit, and switching notes or closing the window asks before dropping them
- The line above the search box tells whether everything is backed up; `⋯` holds 立即備份, pause, add and remove folder

## Requirements

- Rust toolchain
- `git` on PATH
- `qmd` on PATH (optional; the built-in note list search works without it)

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
