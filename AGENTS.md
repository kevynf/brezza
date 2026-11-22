# brezza

Windows desktop Git client written in Rust with egui/eframe.

## Commands
- Build: `cargo build`. Open a repository: `cargo run -- <path>`. Release build: `cargo build --release`.
- After adding or removing `icons::*` references, run `uv run scripts/subset_icons.py` to regenerate `assets/lucide.ttf` and `src/ui/icons.rs`.
- Before committing, run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`. CI repeats them but does not replace running them locally.

## Conventions
- Check the egui and gix versions in `Cargo.toml` and read those crate sources instead of relying on memory; their APIs change between versions.
- Read git with gix in `src/git/repo.rs` and write only through the git CLI in `src/git/cli.rs`, the only place that spawns processes (enforced by `clippy.toml`).
- Read and write working-tree files only in `src/worktree.rs`; `src/tab.rs` owns edit state and policy, not file I/O.
- Keep git, diff and highlighting work off the UI thread: send it to the worker in `src/git/worker.rs` through the messages in `src/git/protocol.rs`.
- Create colors and touch egui `Style`/`Visuals` only in `src/ui/theme.rs` (enforced by `clippy.toml`). In views, take colors from `theme::palette(ui.visuals())`.
- Compose views from the components in `src/ui/chrome.rs`; add a component there instead of styling inside a view.
- Paint custom shapes only in `src/ui/graph_view.rs` and `src/ui/diff_view.rs`.
- Use `theme::size` for every fixed dimension, `theme::space` for spacing, and only `radius::S`, `M` and `L` for corners.
- Use three text roles: caption `.small()`, body by default, code `.monospace()`. Use only regular or `.strong()` weight; do not use italics, `.size(...)` or uppercase.
- Route user-facing text through `fl!` and add every key to both `i18n/en/brezza.ftl` and `i18n/zh-CN/brezza.ftl`.
- Give icon-only controls a tooltip. Use icon + text only for a view's primary action, dialog buttons and section-wide actions; keep menu items and view tabs text only.
