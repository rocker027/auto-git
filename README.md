# note-sync

Lightweight macOS menu-bar tool for Markdown note folders:

- Closing the window (red button) keeps the app running in the menu bar and backing up; 「顯示視窗」 in the menu bar icon or the Dock icon brings it back. ⌘Q or 「結束」 quits

- Watch multiple folders and periodically `git add` + `git commit` (`update N files`)
- `git init` when a folder is not a repo
- Left: 「＋ 加入資料夾」 above the list, then every Markdown note grouped by folder, newest first. Click a folder title to fold or unfold it; searching shows matches in every folder. The search box filters by name and text as you type
- Optional: with [`qmd`](https://github.com/tobi/qmd) on PATH, matching notes from other qmd collections appear under 「其他地方」
- Right: one note open for editing. It saves on its own about 1 s after you stop typing, and before switching notes, closing or quitting; ⌘S saves at once. 「編輯｜預覽」 (⌘E) switches between the editor and rendered Markdown. Autosave never overwrites a note changed elsewhere and never recreates one moved or deleted elsewhere; it stops and offers the newer version, and waits while an input method is still composing. Unsaved edits pause auto-commit
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
