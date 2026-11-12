# note-sync

Lightweight macOS menu-bar tool for Markdown note folders:

- Closing the window (red button) keeps the app running in the menu bar and backing up; 「顯示視窗」 in the menu bar icon or the Dock icon brings it back. ⌘Q or 「結束」 quits
- Watch multiple folders and periodically `git add` + `git commit` (`update N files`)
- Optional auto push, off by default: 「備份」 tab → 「自動推送」. After each backup the current branch is pushed to the upstream it already has (for example `origin/main` on GitHub), whenever it is ahead, so a push that failed is tried again next time. It never forces, pulls, adds a remote or sets an upstream, and sends no tags, submodules or other branches; a folder without an upstream on a remote is only backed up locally. Turning it off also stops the folders not yet pushed in the current run. When the remote has newer commits the push is refused and the status line asks for a `git pull`. The app has no terminal, so Git credentials must already work without a prompt (macOS keychain or an SSH agent); a push is stopped after 120 s
- `git init` when a folder is not a repo. A folder inside another repo (not its top level) is skipped with a warning, so files outside the folder are never committed
- Left: a sidebar with two tabs at its bottom, 「筆記」 (⌘1) and 「備份」 (⌘2); the open note stays on the right in both. In 「筆記」, 「最近更新」 shows the newest 30 Markdown notes across watched folders, ordered by the later of creation and modification time, without duplicate paths. This section starts expanded; folder sections start collapsed. Hover a note for its full path; right-click a note to copy its path or reveal it in Finder. Each folder still lists its Markdown notes, newest modified first. Click a folder title to fold or unfold it; searching shows matches in every folder and hides the recent section to avoid duplicate results. The search box filters by name and text as you type; ⌘F jumps to it from either tab. 「＋ 加入資料夾」 is at the end of the list
- Optional: with [`qmd`](https://github.com/tobi/qmd) on PATH, matching notes from other qmd collections appear under 「其他地方」
- Right: one note open for editing. It saves on its own about 1 s after you stop typing, and before switching notes, closing or quitting; ⌘S saves at once. 「編輯｜預覽｜歷史」 switches between the editor, rendered Markdown (⌘E toggles these two) and the note's backed-up versions. 「歷史」 is read only: pick a version to view it, copy its text, or save it as a new note next to the original; the open note is never changed. Autosave never overwrites a note changed elsewhere and never recreates one moved or deleted elsewhere; it stops and offers the newer version, and waits while an input method is still composing. Unsaved edits pause auto-commit
- The line below the search box tells whether everything is backed up; click it to open the 「備份」 tab. Its time is when the newest version was made (hover for the last check), and 「已存檔，等下次備份」 means a save is not in a version yet; with auto push on it reads 「已備份並推送」 once every folder has reached its remote, and a failed push shows as a warning while the local version is kept. The 「備份」 tab shows the same state with its full explanation, and holds the 「自動備份」 and 「自動推送」 switches, 「立即備份」, and the watched folders with add and remove

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
