<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brezza-mark-white.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/brezza-mark-black.svg">
    <img src="assets/brezza-mark-black.svg" width="96" alt="brezza logo">
  </picture>
</p>

<h1 align="center">brezza</h1>

<p align="center">
  <strong>A Git client for Windows</strong><br>
  Focused on commit history, diffs, and everyday repository work.
</p>

<p align="center">
  <a href="https://github.com/kevynf/brezza/actions/workflows/ci.yml"><img src="https://github.com/kevynf/brezza/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/kevynf/brezza/releases"><img src="https://img.shields.io/github/v/release/kevynf/brezza?include_prereleases&label=release" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/kevynf/brezza" alt="License"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4?logo=windows&logoColor=white" alt="Windows 10 and 11">
</p>

<p align="center"><a href="README.md">简体中文</a> | English</p>

> [!WARNING]
> **Alpha:** brezza is under active development; features and UI may change. Before destructive actions such as reset, discard, or force push, verify the target and keep backups where needed.

Browse the commit graph, edit working-tree files, stage changes by hunk, manage branches and remotes, and resolve conflicts from merges or rebases without jumping between tools.

## Features

- **History and diffs** — Commit graph, commit details, and commit search; unified or split diff views; flat or tree file lists.
- **Working tree and staging** — Edit working-tree files in place and save dirty buffers before staging; stage and unstage by file or hunk, and discard by file.
- **Commits** — Commit, amend the previous commit, and delete untracked files; diffs are syntax-highlighted.
- **History operations** — Merge, rebase, cherry-pick, revert, reset, and bisect; conflicts can be marked resolved or opened in an external merge tool.
- **Branches and remotes** — Manage branches, tags, remotes, and stashes; fetch, pull, push, force push, and track upstream branches.
- **Desktop experience** — Multiple tabs and a command palette; light, dark, or system theme; English and Simplified Chinese.

## Download and run

Download the latest Windows zip from [Releases](https://github.com/kevynf/brezza/releases). The archive contains a single `brezza.exe`; there is no installer. Extract it and run.

### Requirements

- Windows 10 or 11 (x86_64)
- [Git for Windows](https://git-scm.com/download/win), with `git` available on `PATH`. brezza executes writes through the Git CLI.
- Rust 1.95 or newer, only when building from source.

## Usage

Launch brezza to open an existing repository, clone a remote one, or initialize a new one. Each repository opens in its own tab; click `+` to open another.

You can also open a repository from the command line:

```powershell
.\brezza.exe C:\path\to\repository
```

Press `Ctrl+P` for the command palette. Commits, branches, tags, and stashes have context menus.

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+O` | Open repository |
| `Ctrl+P` | Command palette |
| `F5` | Refresh |
| `Ctrl+S` | Save the working-tree file being edited |
| `Ctrl+Enter` | Commit |
| `F7` / `Shift+F7` | Next / previous change in a diff |
| `Enter` / `Shift+Enter` | Next / previous commit search match |

## Build from source

Rust 1.95 or newer is required.

```powershell
git clone https://github.com/kevynf/brezza.git
cd brezza
cargo build --release
.\target\release\brezza.exe
```

Before submitting changes, run:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

After adding or removing `icons::*` references, run `uv run scripts/subset_icons.py` to regenerate `assets/lucide.ttf` and `src/ui/icons.rs`.

## Implementation

- Repository history and objects are read with [`gix`](https://github.com/GitoxideLabs/gitoxide).
- Writes go through the Git CLI, matching the behavior of the `git` commands users already know.
- Repository reads, diffs, and syntax highlighting run on background threads to keep the UI responsive.

## Feedback

Bug reports and feature requests are welcome through [Issues](https://github.com/kevynf/brezza/issues).

## License

This project is licensed under the [MIT License](LICENSE).

The bundled Lucide icon font is distributed under the ISC license; see [`assets/LICENSE-lucide`](assets/LICENSE-lucide).
